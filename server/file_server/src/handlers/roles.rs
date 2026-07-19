use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn error_response(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(serde_json::json!({ "status": "error", "message": message.into() })),
    )
        .into_response()
}

/// Emails the resident that a designation was granted/revoked. Best-effort — logged on
/// failure, never allowed to fail the assign/revoke action itself.
async fn notify_designation_change(user_email: &str, designation_code: &str, granted: bool) {
    let verb = if granted { "granted" } else { "revoked" };
    let payload = email::Email::Generic {
        to: user_email.to_string(),
        subject: "Your Raheja Vistas role has changed".to_string(),
        body: format!(
            "Hello,\n\nThe '{}' role has been {} on your account.\n\n\
             Please log in to the Raheja Vistas portal to see what this changes for you.\n",
            designation_code, verb
        ),
    };
    if let Err(e) = email::send_email(&payload).await {
        eprintln!(
            "[NOTIFICATION] Designation-change email failed for {}: {}",
            user_email, e
        );
    }
}

/// Blocks self-edits and touching system designations (e.g. super_admin).
async fn guard_designation_change(
    state: &AppState,
    caller_user_id: i32,
    target_user_id: i32,
    designation_code: &str,
) -> Result<(), axum::response::Response> {
    if target_user_id == caller_user_id {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "You cannot modify your own designations. Another admin must make this change.",
        ));
    }

    let is_system = state
        .permissions_manager
        .is_system_designation(designation_code)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("Unknown designation: {}", designation_code),
            )
        })?;

    if is_system {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            format!(
                "'{}' is a system designation and cannot be assigned or revoked.",
                designation_code
            ),
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/admin/roles/designations
/// Lists all designations, for populating the assign dropdown.
pub async fn list_designations(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> impl IntoResponse {
    match state.permissions_manager.list_designations().await {
        Ok(designations) => (StatusCode::OK, Json(designations)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct DesignationChangePayload {
    pub user_id: i32,
    pub designation_code: String,
}

/// POST /api/v1/admin/roles/designations/assign
pub async fn assign_designation(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<DesignationChangePayload>,
) -> impl IntoResponse {
    if let Err(resp) = guard_designation_change(
        &state,
        caller.user_id,
        payload.user_id,
        &payload.designation_code,
    )
    .await
    {
        return resp;
    }

    match state
        .permissions_manager
        .assign_designation(
            payload.user_id,
            &payload.designation_code,
            Some(caller.user_id),
        )
        .await
    {
        Ok(_) => {
            if let Ok(Some(user)) = state
                .permissions_manager
                .get_user_by_id(payload.user_id)
                .await
            {
                notify_designation_change(&user.email, &payload.designation_code, true).await;
            }
            (
                StatusCode::OK,
                Json(
                    serde_json::json!({ "status": "success", "message": "Designation assigned." }),
                ),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// POST /api/v1/admin/roles/designations/revoke
pub async fn revoke_designation(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<DesignationChangePayload>,
) -> impl IntoResponse {
    if let Err(resp) = guard_designation_change(
        &state,
        caller.user_id,
        payload.user_id,
        &payload.designation_code,
    )
    .await
    {
        return resp;
    }

    match state
        .permissions_manager
        .revoke_designation(
            payload.user_id,
            &payload.designation_code,
            Some(caller.user_id),
        )
        .await
    {
        Ok(_) => {
            if let Ok(Some(user)) = state
                .permissions_manager
                .get_user_by_id(payload.user_id)
                .await
            {
                notify_designation_change(&user.email, &payload.designation_code, false).await;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "success", "message": "Designation revoked." })),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// POST /api/v1/admin/roles/permissions
/// Lists the full (static) permission catalog, for building the designation permission matrix.
pub async fn list_permissions(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> impl IntoResponse {
    match state.permissions_manager.list_permissions().await {
        Ok(permissions) => (StatusCode::OK, Json(permissions)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateDesignationPayload {
    pub code: String,
    pub label: String,
}

/// POST /api/v1/admin/roles/designations/create
pub async fn create_designation(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<CreateDesignationPayload>,
) -> impl IntoResponse {
    let code = payload.code.trim().to_lowercase().replace(' ', "_");
    let label = payload.label.trim().to_string();

    if code.is_empty() || label.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Code and label are required.");
    }

    match state
        .permissions_manager
        .create_designation(&code, &label, Some(caller.user_id))
        .await
    {
        Ok(designation) => (StatusCode::CREATED, Json(designation)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(serde::Deserialize)]
pub struct RenameDesignationPayload {
    pub code: String,
    pub label: String,
}

/// POST /api/v1/admin/roles/designations/rename
pub async fn rename_designation(
    State(state): State<AppState>,
    _session_user: SessionUser,
    Json(payload): Json<RenameDesignationPayload>,
) -> impl IntoResponse {
    let label = payload.label.trim().to_string();
    if label.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Label is required.");
    }
    match state
        .permissions_manager
        .rename_designation(&payload.code, &label)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Designation renamed." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(serde::Deserialize)]
pub struct DeleteDesignationPayload {
    pub code: String,
}

/// POST /api/v1/admin/roles/designations/delete
pub async fn delete_designation(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<DeleteDesignationPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .delete_designation(&payload.code, Some(caller.user_id))
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Designation deleted." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(serde::Deserialize)]
pub struct ToggleDesignationPermissionPayload {
    pub designation_code: String,
    pub permission_code: String,
    pub enabled: bool,
}

/// POST /api/v1/admin/roles/designations/permissions/toggle
/// Grants or revokes a permission for a designation.
pub async fn toggle_designation_permission(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<ToggleDesignationPermissionPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .is_system_designation(&payload.designation_code)
        .await
    {
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("Unknown designation: {}", payload.designation_code),
            )
        }
        Ok(Some(true)) => {
            return error_response(
                StatusCode::FORBIDDEN,
                format!(
                    "'{}' is a system designation and its permissions cannot be edited.",
                    payload.designation_code
                ),
            )
        }
        Ok(Some(false)) => {}
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }

    // A designation.manage holder must never be able to grant/revoke permissions on a
    // designation they themselves currently hold — otherwise they could hand their own
    // designation any permission at all (self-escalation), bypassing the assign/revoke
    // self-edit guard entirely.
    if caller.has_designation(&payload.designation_code) {
        return error_response(
            StatusCode::FORBIDDEN,
            "You cannot modify the permissions of a designation you hold yourself. \
             Another admin must make this change.",
        );
    }

    match state
        .permissions_manager
        .toggle_designation_permission(
            &payload.designation_code,
            &payload.permission_code,
            payload.enabled,
            Some(caller.user_id),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Permission updated." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// POST /api/v1/admin/roles/document-category-designations
/// Lists the current document-category-to-designation access matrix.
pub async fn list_document_category_designations(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> impl IntoResponse {
    match state.document_manager.list_category_designations().await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(serde::Deserialize)]
pub struct SetCategoryDesignationPayload {
    pub category: String,
    pub designation_code: String,
    pub can_read: bool,
    pub can_manage: bool,
}

/// POST /api/v1/admin/roles/document-category-designations/toggle
pub async fn toggle_document_category_designation(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<SetCategoryDesignationPayload>,
) -> impl IntoResponse {
    match state
        .document_manager
        .set_category_designation(
            &payload.category,
            &payload.designation_code,
            payload.can_read,
            payload.can_manage,
            Some(caller.user_id),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Mapping updated." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// POST /api/v1/admin/roles/document-categories
/// Lists all document categories, including the two protected system ones.
pub async fn list_document_categories(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> impl IntoResponse {
    match state.document_manager.list_document_categories().await {
        Ok(categories) => (StatusCode::OK, Json(categories)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateDocumentCategoryPayload {
    pub code: String,
    pub label: String,
}

/// POST /api/v1/admin/roles/document-categories/create
pub async fn create_document_category(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<CreateDocumentCategoryPayload>,
) -> impl IntoResponse {
    let code = payload.code.trim().to_uppercase().replace(' ', "_");
    let label = payload.label.trim().to_string();

    if code.is_empty() || label.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Code and label are required.");
    }

    match state
        .document_manager
        .create_document_category(&code, &label, Some(caller.user_id))
        .await
    {
        Ok(category) => (StatusCode::CREATED, Json(category)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(serde::Deserialize)]
pub struct RenameDocumentCategoryPayload {
    pub code: String,
    pub label: String,
}

/// POST /api/v1/admin/roles/document-categories/rename
pub async fn rename_document_category(
    State(state): State<AppState>,
    _session_user: SessionUser,
    Json(payload): Json<RenameDocumentCategoryPayload>,
) -> impl IntoResponse {
    let label = payload.label.trim().to_string();
    if label.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Label is required.");
    }
    match state
        .document_manager
        .rename_document_category(&payload.code, &label)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Category renamed." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(serde::Deserialize)]
pub struct DeleteDocumentCategoryPayload {
    pub code: String,
}

/// POST /api/v1/admin/roles/document-categories/delete
pub async fn delete_document_category(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<DeleteDocumentCategoryPayload>,
) -> impl IntoResponse {
    match state
        .document_manager
        .delete_document_category(&payload.code, Some(caller.user_id))
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Category deleted." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateDocumentSubcategoryPayload {
    pub category: String,
    pub code: String,
    pub label: String,
}

/// POST /api/v1/admin/roles/document-subcategories/create
pub async fn create_document_subcategory(
    State(state): State<AppState>,
    SessionUser(caller): SessionUser,
    Json(payload): Json<CreateDocumentSubcategoryPayload>,
) -> impl IntoResponse {
    let code = payload.code.trim().to_uppercase().replace(' ', "_");
    let label = payload.label.trim().to_string();

    if code.is_empty() || label.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Code and label are required.");
    }

    match state
        .document_manager
        .create_document_subcategory(&payload.category, &code, &label, Some(caller.user_id))
        .await
    {
        Ok(subcategory) => (StatusCode::CREATED, Json(subcategory)).into_response(),
        Err(e) => error_response(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(serde::Deserialize)]
pub struct RenameDocumentSubcategoryPayload {
    pub subcategory_id: i32,
    pub label: String,
}

/// POST /api/v1/admin/roles/document-subcategories/rename
pub async fn rename_document_subcategory(
    State(state): State<AppState>,
    _session_user: SessionUser,
    Json(payload): Json<RenameDocumentSubcategoryPayload>,
) -> impl IntoResponse {
    let label = payload.label.trim().to_string();
    if label.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Label is required.");
    }
    match state
        .document_manager
        .rename_document_subcategory(payload.subcategory_id, &label)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Subcategory renamed." })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::BAD_REQUEST, e),
    }
}
