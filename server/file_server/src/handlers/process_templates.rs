//! Admin authoring API for versioned process templates (the builder UI). Separate
//! from `handlers::process`, which only ever deals with already-pinned process
//! instances — this is the only place `process_templates`/`process_template_versions`
//! get written to.

use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::template_store::TemplateDraft;
use serde::{Deserialize, Serialize};

type ApiError = (StatusCode, Json<String>);

fn internal_err(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string()))
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(e.to_string()))
}

/// POST /api/v1/admin/process-templates/list
pub async fn list(
    State(state): State<AppState>,
    _user: SessionUser,
) -> Result<Json<Vec<process::template_store::TemplateSummary>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let summaries = process::template_store::list_templates(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(summaries))
}

#[derive(Deserialize)]
pub struct GetVersionPayload {
    pub version_id: i32,
}

/// POST /api/v1/admin/process-templates/get-version
/// Loads one version's full tree, for opening it in the builder.
pub async fn get_version(
    State(state): State<AppState>,
    _user: SessionUser,
    Json(payload): Json<GetVersionPayload>,
) -> Result<Json<process::ProcessTemplate>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template =
        process::template_store::get_template_version_detail(&mut conn, payload.version_id)
            .await
            .map_err(bad_request)?
            .ok_or_else(|| (StatusCode::NOT_FOUND, Json("Version not found".to_string())))?;
    Ok(Json(template))
}

#[derive(Serialize)]
pub struct SaveResponse {
    pub version_id: i32,
}

/// POST /api/v1/admin/process-templates/save
/// The upsert: `draft.template_id: None` (or unknown) creates a brand-new template;
/// an existing `template_id` appends a new draft version. Never auto-activates.
pub async fn save(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(draft): Json<TemplateDraft>,
) -> Result<Json<SaveResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let version_id = process::template_store::save_template_version(&mut conn, draft, user.user_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(SaveResponse { version_id }))
}

#[derive(Deserialize)]
pub struct ActivatePayload {
    pub template_id: i32,
    pub version_id: i32,
}

/// POST /api/v1/admin/process-templates/activate
pub async fn activate(
    State(state): State<AppState>,
    _user: SessionUser,
    Json(payload): Json<ActivatePayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    process::template_store::activate_template_version(
        &mut conn,
        payload.template_id,
        payload.version_id,
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /api/v1/process-templates/available
/// Resident-facing: which templates can currently be started (have an active version).
/// No admin permission required — this is what populates the "Start Process" picker.
pub async fn available(
    State(state): State<AppState>,
    _user: SessionUser,
) -> Result<Json<Vec<AvailableTemplate>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let templates = process::template_store::list_startable_templates(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(
        templates
            .into_iter()
            .map(|(template_id, name)| AvailableTemplate { template_id, name })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct AvailableTemplate {
    pub template_id: i32,
    pub name: String,
}
