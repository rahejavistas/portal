use crate::extractors::generate_token;
use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rand::{thread_rng, Rng};

#[derive(serde::Deserialize)]
pub struct LoginPayload {
    email: String,
    password: String,
}

#[derive(serde::Serialize)]
pub struct LoginResponse {
    status: String,
    user: Option<authentication::User>,
    token: Option<String>,
    message: Option<String>,
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .check_authenticated(&payload.email, &payload.password)
        .await
    {
        Ok(authentication::AuthResult::Success(user)) => {
            match generate_token(&user, &state.jwt_secret, state.jwt_expiry) {
                Ok(token) => (
                    StatusCode::OK,
                    Json(LoginResponse {
                        status: "success".to_string(),
                        user: Some(user),
                        token: Some(token),
                        message: None,
                    }),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LoginResponse {
                        status: "error".to_string(),
                        user: None,
                        token: None,
                        message: Some(format!("Token generation failed: {}", e)),
                    }),
                )
                    .into_response(),
            }
        }
        Ok(authentication::AuthResult::InvalidCredentials) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some("Invalid email or password".to_string()),
            }),
        )
            .into_response(),
        Ok(authentication::AuthResult::AccountLocked) => (
            StatusCode::FORBIDDEN,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(
                    "Account locked due to too many failed attempts. Please reset your password."
                        .to_string(),
                ),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(format!("Database error: {}", e)),
            }),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ForgotPasswordPayload {
    email: String,
}

#[derive(serde::Serialize)]
pub struct ForgotPasswordResponse {
    status: String,
    reset_link: String,
    message: String,
}

pub async fn forgot_password(
    State(state): State<AppState>,
    Json(payload): Json<ForgotPasswordPayload>,
) -> Response {
    // Generate a secure 32-character alphanumeric reset token
    let token: String = thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let email = payload.email.trim().to_lowercase();

    match state.permissions_manager.email_exists(&email).await {
        Ok(true) => {
            // User exists, set the reset token
            if let Err(e) = state
                .permissions_manager
                .create_reset_token(
                    &email,
                    &token,
                    state.forgot_password_expiry.as_secs() as i64,
                )
                .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Failed to set reset token: {}", e)
                    })),
                )
                    .into_response();
            }
        }
        Ok(false) => {
            // Unregistered email! Do not create dummy data, just return NOT_FOUND
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "status": "error",
                    "message": "Email address not registered. Please contact your property administrator."
                })),
            ).into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response();
        }
    }

    let reset_link = format!("http://localhost:4000/reset-password?token={}", token);

    let email_payload = email::Email::ForgotPassword {
        to: email.clone(),
        subject: "Raheja Vistas Documents Portal: Password Setup / Reset".to_string(),
        body: format!(
            "Dear User,\n\nPlease use the following link to setup/reset your password: {}",
            reset_link
        ),
    };

    // Log the reset link in server logs for testing/recovery
    tracing::info!(
        "Generated password setup/reset link for '{}': {}",
        email,
        reset_link
    );

    // Call the generic email carrier, but don't fail the request if it fails
    if let Err(e) = email::send_email(&email_payload).await {
        tracing::error!("Email send error: {}", e);
    }

    (
        StatusCode::OK,
        Json(ForgotPasswordResponse {
            status: "success".to_string(),
            reset_link,
            message: "Password setup/reset link generated successfully".to_string(),
        }),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct ResetPasswordPayload {
    token: String,
    password: String,
}

pub async fn reset_password(
    State(state): State<AppState>,
    Json(payload): Json<ResetPasswordPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .reset_password_with_token(&payload.token, &payload.password)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "success",
                "message": "Password updated successfully"
            })),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid or expired password reset token"
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Reset failed: {}", e)
            })),
        )
            .into_response(),
    }
}

pub async fn profile(
    State(state): State<AppState>,
    crate::extractors::SessionUser(session_user): crate::extractors::SessionUser,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .get_user_by_id(session_user.user_id)
        .await
    {
        Ok(Some(user)) => (StatusCode::OK, Json(user)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "User not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}
