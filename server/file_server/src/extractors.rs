use crate::state::AppState;
use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(dead_code)]
pub struct SessionUser(pub authentication::User);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub email: String,
    pub wing: String,
    pub flat_number: String,
    pub roles: Vec<authentication::UserRole>,
    pub exp: usize, // expiration timestamp
}

/// Helper to generate a stateless JWT token using the configured secret.
pub fn generate_token(
    user: &authentication::User,
    secret: &str,
    expiry: std::time::Duration,
) -> Result<String, jsonwebtoken::errors::Error> {
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + (expiry.as_secs() as usize);

    let claims = Claims {
        sub: user.user_id.to_string(),
        email: user.email.clone(),
        wing: user.wing.clone(),
        flat_number: user.flat_number.clone(),
        roles: user.roles.clone(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

#[async_trait]
impl<S> FromRequestParts<S> for SessionUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let user_id = if let Some(claims) = parts.extensions.get::<Claims>() {
            claims.sub.parse::<i32>().unwrap_or(0)
        } else {
            let auth_header = parts
                .headers
                .get("authorization")
                .or_else(|| parts.headers.get("Authorization"))
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "));

            let token = match auth_header {
                Some(t) => t,
                None => return Err(StatusCode::UNAUTHORIZED),
            };

            // Cryptographically decode and validate the JWT token using AppState secret
            let token_data = match decode::<Claims>(
                token,
                &DecodingKey::from_secret(app_state.jwt_secret.as_bytes()),
                &Validation::default(),
            ) {
                Ok(data) => data,
                Err(_) => return Err(StatusCode::UNAUTHORIZED),
            };

            token_data.claims.sub.parse::<i32>().unwrap_or(0)
        };

        let user = match app_state.permissions_manager.get_user_by_id(user_id).await {
            Ok(Some(u)) => u,
            _ => return Err(StatusCode::UNAUTHORIZED),
        };

        Ok(SessionUser(user))
    }
}
