use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base::flat::FlatIdentifier;

#[derive(serde::Deserialize)]
pub struct CreateUserPayload {
    pub email: String,
    pub wing: String,
    pub flat_number: String,
}

/// List all users.
pub async fn list_users(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> impl IntoResponse {
    match state.permissions_manager.get_all_users().await {
        Ok(users) => (StatusCode::OK, Json(users)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Database error: {}", e)
            })),
        )
            .into_response(),
    }
}

/// Create new user.
pub async fn create_user(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<CreateUserPayload>,
) -> impl IntoResponse {
    let email = payload.email.trim().to_lowercase();

    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "All fields (email, wing, flat_number) are required and cannot be empty."
            })),
        )
            .into_response();
    }

    let flat = match FlatIdentifier::new(&payload.wing, &payload.flat_number) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid flat identifier: {}", e)
                })),
            )
                .into_response();
        }
    };

    match state
        .permissions_manager
        .create_user_with_credentials(
            &email,
            None,
            flat.wing(),
            flat.flat_number(),
            Some(caller.user_id),
        )
        .await
    {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Failed to create user: {}", e)
            })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct RevokeFlatPayload {
    pub user_id: i32,
    pub wing: String,
    pub flat_number: String,
}

pub async fn admin_revoke_flat(
    State(state): State<AppState>,
    _session_user: SessionUser,
    Json(payload): Json<RevokeFlatPayload>,
) -> impl IntoResponse {
    let flat = match FlatIdentifier::new(&payload.wing, &payload.flat_number) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid flat identifier: {}", e)
                })),
            )
                .into_response();
        }
    };

    match state
        .permissions_manager
        .revoke_flat_ownership(payload.user_id, flat.wing(), flat.flat_number())
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "success",
                "message": "Flat mapping revoked successfully."
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Failed to revoke flat mapping: {}", e)
            })),
        )
            .into_response(),
    }
}
