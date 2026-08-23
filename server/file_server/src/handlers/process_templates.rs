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

#[derive(Deserialize)]
pub struct DeactivatePayload {
    pub template_id: i32,
}

/// POST /api/v1/admin/process-templates/deactivate
/// Clears the active version so no resident can start a new process against this
/// template — in-progress processes stay pinned to their own version, unaffected.
pub async fn deactivate(
    State(state): State<AppState>,
    _user: SessionUser,
    Json(payload): Json<DeactivatePayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    process::template_store::deactivate_template(&mut conn, payload.template_id)
        .await
        .map_err(bad_request)?;
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

/// POST /api/v1/admin/process-templates/holidays/list
pub async fn list_holidays(
    State(state): State<AppState>,
    _user: SessionUser,
) -> Result<Json<Vec<process::db::PublicHoliday>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let holidays = process::db::list_public_holidays(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(holidays))
}

#[derive(Deserialize)]
pub struct AddHolidayPayload {
    pub holiday_date: chrono::NaiveDate,
    pub description: String,
}

/// POST /api/v1/admin/process-templates/holidays/add
pub async fn add_holiday(
    State(state): State<AppState>,
    _user: SessionUser,
    Json(payload): Json<AddHolidayPayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if payload.description.trim().is_empty() {
        return Err(bad_request("description cannot be empty"));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    process::db::add_public_holiday(&mut conn, payload.holiday_date, payload.description.trim())
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(Deserialize)]
pub struct RemoveHolidayPayload {
    pub holiday_date: chrono::NaiveDate,
}

/// POST /api/v1/admin/process-templates/holidays/remove
pub async fn remove_holiday(
    State(state): State<AppState>,
    _user: SessionUser,
    Json(payload): Json<RemoveHolidayPayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    process::db::remove_public_holiday(&mut conn, payload.holiday_date)
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /api/v1/admin/process-templates/tat-report — average actual turnaround per
/// template, over every process that has closed with SLA tracking on.
pub async fn tat_report(
    State(state): State<AppState>,
    _user: SessionUser,
) -> Result<Json<Vec<process::db::TemplateTatSummary>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let summary = process::db::tat_actual_summary_by_template(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(summary))
}
