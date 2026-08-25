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
                "message": ({ eprintln!("Admin handler DB error: {}", e); "An internal error occurred. Please try again." })
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
    if !email.contains('@') || !email.contains('.') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid email address."
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
                "message": ({ eprintln!("Failed to create user: {}", e); "Could not create user. Please try again." })
            })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateStaffPayload {
    pub email: String,
}

/// Create a staff account — no flat ownership, auto-assigned the `staff` designation.
pub async fn create_staff(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<CreateStaffPayload>,
) -> impl IntoResponse {
    let email = payload.email.trim().to_lowercase();

    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Email is required."
            })),
        )
            .into_response();
    }

    match state
        .permissions_manager
        .create_staff_user(&email, Some(caller.user_id))
        .await
    {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": ({ eprintln!("Failed to create staff account: {}", e); "Could not create staff account. Please try again." })
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

#[derive(serde::Deserialize)]
pub struct LookupFlatOwnersPayload {
    pub wing: String,
    pub flat_number: String,
}

/// Previews who owns a flat before mapping a document to it. Invalid/incomplete input
/// returns an empty list rather than an error — a soft preview, not a validated write.
/// Only used for resident-specific document registration, so scoped to that category
/// specifically — managing an unrelated category (e.g. notices) isn't enough to look up
/// owner emails for an arbitrary flat.
pub async fn lookup_flat_owners(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<LookupFlatOwnersPayload>,
) -> impl IntoResponse {
    let can_manage_resident_specific =
        match state.document_manager.manageable_categories(&user).await {
            Ok(None) => true,
            Ok(Some(categories)) => {
                categories.contains(crate::document::RESIDENT_SPECIFIC_CATEGORY)
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "status": "error", "message": e })),
                )
                    .into_response()
            }
        };
    if !can_manage_resident_specific {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "status": "error",
                "message": "You do not have permission to look up flat owners."
            })),
        )
            .into_response();
    }

    let emails = match FlatIdentifier::new(&payload.wing, &payload.flat_number) {
        Ok(flat) => state
            .permissions_manager
            .active_owner_emails_for_flat(flat.wing(), flat.flat_number())
            .await
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({ "emails": emails })),
    )
        .into_response()
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
                "message": ({ eprintln!("Failed to revoke flat mapping: {}", e); "Could not revoke flat mapping. Please try again." })
            })),
        )
            .into_response(),
    }
}
