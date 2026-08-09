use crate::extractors::Claims;
use crate::state::AppState;
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderValue, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};
use jsonwebtoken::{decode, DecodingKey, Validation};
use opentelemetry::{global, KeyValue};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};

pub struct RateLimiters {
    pub enabled: bool,
    pub ip_limiter: RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>,
    pub user_limiter: RateLimiter<i32, DefaultKeyedStateStore<i32>, DefaultClock>,
    pub admin_limiter: RateLimiter<i32, DefaultKeyedStateStore<i32>, DefaultClock>,
}

impl RateLimiters {
    pub fn new(enabled: bool, ip_per_min: u32, user_per_min: u32, admin_per_min: u32) -> Self {
        let ip_quota = Quota::per_minute(NonZeroU32::new(ip_per_min.max(1)).unwrap());
        let user_quota = Quota::per_minute(NonZeroU32::new(user_per_min.max(1)).unwrap());
        let admin_quota = Quota::per_minute(NonZeroU32::new(admin_per_min.max(1)).unwrap());

        Self {
            enabled,
            ip_limiter: RateLimiter::keyed(ip_quota),
            user_limiter: RateLimiter::keyed(user_quota),
            admin_limiter: RateLimiter::keyed(admin_quota),
        }
    }
}

pub async fn rate_limit_middleware<B>(
    State(state): State<AppState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    mut req: Request<B>,
    next: Next<B>,
) -> Result<Response, StatusCode> {
    let ip = if let Some(ConnectInfo(addr)) = connect_info {
        addr.ip()
    } else {
        // Check X-Forwarded-For header if ConnectInfo is not present (e.g. on AWS Lambda)
        req.headers()
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
            .unwrap_or_else(|| IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)))
    };

    if state.rate_limiters.enabled && state.rate_limiters.ip_limiter.check_key(&ip).is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let auth_header = req
        .headers()
        .get("authorization")
        .or_else(|| req.headers().get("Authorization"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    if let Some(token) = auth_header {
        if let Ok(data) = decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
            &Validation::default(),
        ) {
            let user_id = data.claims.sub.parse::<i32>().unwrap_or(0);
            let is_admin = data.claims.designations.iter().any(|d| d == "super_admin");

            if state.rate_limiters.enabled {
                let limiter = if is_admin {
                    &state.rate_limiters.admin_limiter
                } else {
                    &state.rate_limiters.user_limiter
                };
                if limiter.check_key(&user_id).is_err() {
                    return Err(StatusCode::TOO_MANY_REQUESTS);
                }
            }

            req.extensions_mut().insert(data.claims.clone());
        }
    }

    Ok(next.run(req).await)
}

/// Configures and returns standard production-ready CORS layer.
pub fn get_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

/// Dynamic middleware to inject browser security headers into every response.
pub async fn add_security_headers<B>(req: Request<B>, next: Next<B>) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );
    // CSP: inline scripts/styles are required for the current single-file HTML architecture.
    // 'unsafe-inline' is intentional here. Migrate to external JS + nonces to harden further.
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' 'unsafe-inline'; \
             style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
             font-src https://fonts.gstatic.com; \
             img-src 'self' data:; \
             connect-src 'self'; \
             object-src 'none'; \
             base-uri 'self'; \
             form-action 'self'",
        ),
    );
    headers.insert(
        "Strict-Transport-Security",
        HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
    );
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));

    response
}

/// Request logging middleware to print requests and headers.
pub async fn log_request<B>(req: Request<B>, next: Next<B>) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let method_str = method.to_string();

    let start = Instant::now();
    let response = next.run(req).await;
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let meter = global::meter("file_server_api");

    let counter = meter.f64_counter("api_requests_total").init();
    counter.add(
        1.0,
        &[
            KeyValue::new("method", method_str.clone()),
            KeyValue::new("route", path.clone()),
            KeyValue::new("status_code", status.clone()),
        ],
    );

    let histogram = meter.f64_histogram("api_request_duration_seconds").init();
    histogram.record(
        duration,
        &[
            KeyValue::new("method", method_str),
            KeyValue::new("route", path),
        ],
    );

    response
}

/// Rejects the request if accounting isn't enabled for the society — real
/// server-side enforcement, not just UI hiding. Society Profile is exempt (super
/// admin needs it before accounting is turned on).
pub async fn require_accounting_enabled<B>(
    State(state): State<AppState>,
    req: Request<B>,
    next: Next<B>,
) -> Response
where
    B: Send + 'static,
{
    if !state.accounting_enabled {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "status": "error",
                "message": "Accounting is not enabled for this society."
            })),
        )
            .into_response();
    }
    next.run(req).await
}

/// Requires the caller to hold at least one permission in `required`.
pub fn require_any_of<B>(
    required: &'static [authentication::Permission],
) -> impl Fn(
    crate::extractors::SessionUser,
    Request<B>,
    Next<B>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
where
    B: Send + 'static,
{
    move |session_user: crate::extractors::SessionUser, req: Request<B>, next: Next<B>| {
        Box::pin(async move {
            if !required.iter().any(|p| session_user.0.has_permission(*p)) {
                let codes: Vec<&str> = required.iter().map(|p| p.code()).collect();
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!(
                            "Access denied: one of {:?} permission(s) required.",
                            codes
                        )
                    })),
                )
                    .into_response();
            }
            next.run(req).await
        })
    }
}
