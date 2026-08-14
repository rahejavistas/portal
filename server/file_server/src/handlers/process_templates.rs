//! Admin authoring API for versioned process templates. The only place
//! process_templates/process_template_versions get written to.

use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::template_store::{TemplateDraft, TemplateStoreError};
use serde::{Deserialize, Serialize};

fn map_store_err(e: TemplateStoreError) -> ApiError {
    match e {
        TemplateStoreError::NotFound(msg) => bad_request(msg),
        TemplateStoreError::Db(_) | TemplateStoreError::Serialize(_) => internal_err(e),
    }
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
            .map_err(map_store_err)?
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
        .map_err(map_store_err)?;
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
    .map_err(map_store_err)?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /api/v1/process-templates/available
/// Resident-facing: templates with an active version, for the "Start Process" picker.
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
