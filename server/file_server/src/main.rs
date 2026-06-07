use authentication::PermissionsManager;
use axum::{
    extract::DefaultBodyLimit,
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;

use file_server::config::AppConfig;
use file_server::state::AppState;
use file_server::{document, handlers, middleware};

#[tokio::main]
async fn main() {
    let config = AppConfig::load_config();

    if let Err(e) = telemetry::init_telemetry(
        config.telemetry.otlp_endpoint.clone(),
        config.telemetry.otlp_username.clone(),
        config.telemetry.otlp_password.clone(),
    ) {
        eprintln!("Warning: Failed to initialize telemetry: {}", e);
    }

    // Initialize Postgres connection pool
    let pg_pool = base::postgres::create_pool(&config.get_postgres_config())
        .await
        .expect("Failed to create Postgres pool");

    let s3_accessor = base::s3::S3Accessor::new(&config.s3).await;

    // Initialize PermissionsManager
    let permissions_manager = Arc::new(PermissionsManager::new(pg_pool.clone()));

    let document_manager = Arc::new(document::DocumentManager::new(
        pg_pool.clone(),
        s3_accessor.clone(),
    ));

    // Seed admin credentials programmatically on startup in a separate, clean helper
    seed_admin_user_or_die(&permissions_manager, &config.admin_email).await;

    let rate_limiters = Arc::new(middleware::RateLimiters::new(
        config.rate_limit.ip_per_min,
        config.rate_limit.user_per_min,
        config.rate_limit.admin_per_min,
    ));
    println!("DEBUG user_per_min = {}", config.rate_limit.user_per_min);

    // Initialize shared application state (completely stateless!)
    let state = AppState {
        permissions_manager,
        document_manager,
        jwt_secret: config.jwt_secret.clone(),
        jwt_expiry: config.jwt_expiry,
        forgot_password_expiry: config.forgot_password_expiry,
        rate_limiters,
        log_reset_link: config.log_reset_link.unwrap_or(false),
    };

    macro_rules! make_app {
        ($state:expr) => {{
            let admin_routes = Router::new()
                .route("/users/list", post(handlers::admin::list_users))
                .route("/users/create", post(handlers::admin::create_user))
                .route("/users/roles", post(handlers::admin::update_role))
                .route(
                    "/users/revoke-flat",
                    post(handlers::admin::admin_revoke_flat),
                )
                .route(
                    "/documents/list",
                    post(handlers::documents::admin_list_documents),
                )
                .route(
                    "/documents/create",
                    post(handlers::documents::admin_create_document),
                )
                .route(
                    "/documents/:id/get",
                    post(handlers::documents::admin_get_document),
                )
                .route(
                    "/documents/:id/update",
                    post(handlers::documents::admin_update_document),
                )
                .route(
                    "/documents/:id/delete",
                    post(handlers::documents::admin_delete_document),
                )
                .route(
                    "/process/review",
                    post(handlers::process::admin_review_step),
                )
                .route(
                    "/process/revoke",
                    post(handlers::process::admin_revoke_step),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::admin_guard,
                ));

            Router::new()
                .route("/health", get(handlers::health::health_check))
                .route("/api/v1/login", post(handlers::auth::login))
                .route(
                    "/api/v1/forgot-password",
                    post(handlers::auth::forgot_password),
                )
                .route(
                    "/api/v1/reset-password",
                    post(handlers::auth::reset_password),
                )
                .route("/api/v1/profile", post(handlers::auth::profile))
                .route(
                    "/api/v1/documents",
                    post(handlers::documents::list_documents),
                )
                .route(
                    "/api/v1/documents/:id",
                    post(handlers::documents::get_document),
                )
                .route(
                    "/api/v1/documents/upload",
                    post(handlers::documents::user_create_document),
                )
                .route(
                    "/api/v1/process/list",
                    post(handlers::process::list_user_processes),
                )
                .route(
                    "/api/v1/process/state",
                    post(handlers::process::get_process_state),
                )
                .route(
                    "/api/v1/process/save-step",
                    post(handlers::process::save_process_step),
                )
                .route(
                    "/api/v1/process/upload",
                    post(handlers::process::process_upload_document),
                )
                .route(
                    "/api/v1/process/download-url",
                    post(handlers::process::process_download_url),
                )
                .nest("/api/v1/admin", admin_routes)
                // Standard CORS and custom security headers middleware
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::rate_limit_middleware,
                ))
                .layer(DefaultBodyLimit::max(1024 * 1024 * 1024 + 10 * 1024 * 1024)) // 1GB + 10MB overhead
                .layer(axum_middleware::from_fn(middleware::add_security_headers))
                .layer(axum_middleware::from_fn(middleware::log_request))
                .layer(middleware::get_cors_layer())
                .with_state($state)
        }};
    }

    let is_lambda = std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok();

    if is_lambda {
        println!("Starting in AWS Lambda mode");
        let app = make_app!(state);
        lambda_http::run(app).await.unwrap();
    } else {
        let app = make_app!(state);
        let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
        println!("Server running locally on http://{}", addr);

        axum::Server::bind(&addr)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    }
}

/// Helper function to programmatically seed the admin user on startup if they don't exist.
async fn seed_admin_user_or_die(permissions_manager: &PermissionsManager, email: &str) {
    match permissions_manager.email_exists(email).await {
        Ok(false) => {
            println!(
                "Admin user '{}' not found. Seeding programmatically...",
                email
            );
            match permissions_manager.create_admin_user(email).await {
                Ok(_) => println!("Admin user '{}' successfully seeded.", email),
                Err(e) => eprintln!("Failed to seed admin user: {}", e),
            }
        }
        Ok(true) => {
            println!("Admin user '{}' already exists in database.", email);
        }
        Err(e) => {
            eprintln!("Error checking admin user existence: {}", e);
        }
    }
}
