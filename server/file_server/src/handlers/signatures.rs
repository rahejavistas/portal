use crate::document::SignatureRequirement;
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use uuid::Uuid;

type ApiError = (StatusCode, Json<String>);

fn internal_err(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string()))
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}

async fn load_template(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<process::ProcessTemplate, ApiError> {
    let info = process::db::get_process_info(&mut *conn, process_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| internal_err("Process not found"))?;
    process::ProcessTemplate::load_by_version(&info.template_id).map_err(internal_err)
}

async fn recompute_gating_after_cancel(
    state: &AppState,
    requirement: &SignatureRequirement,
) -> Result<(), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, requirement.process_id).await?;
    process::db::recompute_gating_for_step(
        &mut conn,
        &template,
        requirement.process_id,
        requirement.template_step_id,
    )
    .await
    .map_err(internal_err)
}

async fn attach_replacement_and_recompute_gating(
    state: &AppState,
    requirement: &SignatureRequirement,
    new_document_id: Uuid,
) -> Result<(), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, requirement.process_id).await?;

    // The original document may have already been superseded by a later save (this
    // requirement is stale) — only repoint the field if it's still pointing at it.
    let still_current = process::db::is_current_field_value(
        &mut conn,
        requirement.process_id,
        requirement.template_field_id,
        requirement.document_id,
    )
    .await
    .map_err(internal_err)?;

    if still_current {
        process::db::attach_signed_replacement(
            &mut conn,
            requirement.process_id,
            requirement.template_step_id,
            requirement.template_field_id,
            new_document_id,
        )
        .await
        .map_err(internal_err)?;
    }

    process::db::recompute_gating_for_step(
        &mut conn,
        &template,
        requirement.process_id,
        requirement.template_step_id,
    )
    .await
    .map_err(internal_err)
}

/// POST /api/v1/documents/signatures/queue
pub async fn queue(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> Result<Json<Vec<SignatureRequirement>>, ApiError> {
    let items = state
        .document_manager
        .signature_queue_list()
        .await
        .map_err(internal_err)?;
    Ok(Json(items))
}

/// POST /api/v1/documents/signatures/all
/// Every requirement regardless of status — lets the admin document list build a
/// document_id <-> signed_document_id lineage map in one call instead of a lookup per row.
pub async fn all(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> Result<Json<Vec<SignatureRequirement>>, ApiError> {
    let items = state
        .document_manager
        .signature_list_all()
        .await
        .map_err(internal_err)?;
    Ok(Json(items))
}

#[derive(serde::Deserialize)]
pub struct ForDocumentPayload {
    pub document_id: Uuid,
}

/// POST /api/v1/documents/signatures/for-document
pub async fn for_document(
    State(state): State<AppState>,
    _session_user: SessionUser,
    Json(payload): Json<ForDocumentPayload>,
) -> Result<Json<Option<SignatureRequirement>>, ApiError> {
    let requirement = state
        .document_manager
        .signature_for_document(payload.document_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(requirement))
}

#[derive(serde::Deserialize)]
pub struct ReplaceUploadUrlPayload {
    pub document_id: Uuid,
    pub content_type: String,
}

#[derive(serde::Serialize)]
pub struct ReplaceUploadUrlResponse {
    pub new_document_id: Uuid,
    pub upload_url: String,
}

/// POST /api/v1/admin/documents/signatures/replace-upload-url
pub async fn replace_upload_url(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<ReplaceUploadUrlPayload>,
) -> Result<Json<ReplaceUploadUrlResponse>, ApiError> {
    let (new_document_id, upload_url) = state
        .document_manager
        .signature_replace_upload_url(payload.document_id, &payload.content_type, user.user_id)
        .await
        .map_err(bad_request)?;

    Ok(Json(ReplaceUploadUrlResponse {
        new_document_id,
        upload_url,
    }))
}

#[derive(serde::Deserialize)]
pub struct ReplaceConfirmUploadPayload {
    pub document_id: Uuid,
    pub new_document_id: Uuid,
}

/// POST /api/v1/admin/documents/signatures/replace-confirm-upload
pub async fn replace_confirm_upload(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<ReplaceConfirmUploadPayload>,
) -> Result<Json<SignatureRequirement>, ApiError> {
    let requirement = state
        .document_manager
        .signature_replace_confirm_upload(
            payload.document_id,
            payload.new_document_id,
            user.user_id,
        )
        .await
        .map_err(bad_request)?;

    attach_replacement_and_recompute_gating(&state, &requirement, payload.new_document_id).await?;

    Ok(Json(requirement))
}

#[derive(serde::Deserialize)]
pub struct CancelPayload {
    pub document_id: Uuid,
    pub notes: Option<String>,
}

/// POST /api/v1/documents/signatures/cancel
pub async fn cancel(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<CancelPayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let requirement = state
        .document_manager
        .signature_cancel_requirement(payload.document_id, user.user_id, payload.notes.as_deref())
        .await
        .map_err(bad_request)?;

    recompute_gating_after_cancel(&state, &requirement).await?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
