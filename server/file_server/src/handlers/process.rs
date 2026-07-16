use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::db::{
    get_active_ownerships_for_user, get_admin_comments, get_field_history,
    get_flat_id_for_ownership, get_flat_label_for_ownership, get_process_info, get_process_reviews,
    get_user_active_flats, get_user_email, list_processes_for_user, AdminComment, ProcessInfo,
    ProcessReviewHistory as ReviewHistoryInfo, UserFlatInfo,
};
use process::notification::NotificationContext;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ==========================================
// Request / Response types
// ==========================================

#[derive(Deserialize)]
pub struct ProcessStateRequest {
    pub process_id: Option<Uuid>,
    pub template_id: Option<String>,
    pub flat_id: Option<i32>,
}

#[derive(Serialize)]
pub struct NextActionInfo {
    pub template_step_id: i32,
    pub name: String,
    pub assigned_to: String,
}

#[derive(Clone, Serialize)]
pub struct FieldHistoryEntry {
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub document_title: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct FieldValue {
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub document_title: Option<String>,
}

#[derive(Deserialize)]
pub struct DocumentUrlRequest {
    pub process_id: Uuid,
    pub document_id: Uuid,
}

#[derive(Serialize)]
pub struct DocumentUrlResponse {
    pub download_url: String,
}

#[derive(Serialize)]
pub struct FieldResponse {
    pub template_field_id: i32,
    pub name: String,
    pub r#type: String,
    pub filled_by: String,
    pub is_optional: bool,
    pub value: Option<FieldValue>,
    pub history: Vec<FieldHistoryEntry>,
}

#[derive(Serialize)]
pub struct StepResponse {
    pub template_step_id: i32,
    pub name: String,
    pub status: process::state_machine::StepStatus,
    pub is_review_required: bool,
    pub fields: Vec<FieldResponse>,
    pub substeps: Vec<StepResponse>,
}

#[derive(Serialize)]
pub struct ProcessStateResponse {
    pub process_id: Uuid,
    pub template_id: String,
    pub process_name: String,
    pub ownership_id: Option<i32>,
    pub flat_label: Option<String>,
    pub is_completed: bool,
    pub next_actions: Vec<NextActionInfo>,
    pub reviews: Vec<ReviewHistoryInfo>,
    /// Standalone admin-only comments. Only populated for admin callers;
    /// empty for residents.
    pub admin_comments: Vec<AdminComment>,
    pub steps: Vec<StepResponse>,
}

#[derive(Deserialize)]
pub struct SaveStepRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub fields: Vec<SaveStepFieldInput>,
}

#[derive(Deserialize)]
pub struct SaveStepFieldInput {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct SaveStepResponse {
    pub status: String,
    pub message: String,
    pub step: SavedStepInfo,
}

#[derive(Serialize)]
pub struct SavedStepInfo {
    pub template_step_id: i32,
    pub is_completed: bool,
}

#[derive(Deserialize)]
pub struct AdminReviewRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub is_approved: bool,
    pub notes: Option<String>,
    /// Optional list of substep template_step_ids the admin is flagging for
    /// resident attention. Stored as an annotation on the review record.
    pub flagged_step_ids: Option<Vec<i32>>,
}

#[derive(Serialize)]
pub struct AdminReviewResponse {
    pub status: String,
    pub message: String,
    pub is_completed: bool,
}

#[derive(Deserialize)]
pub struct AdminRevokeRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct AdminRevokeResponse {
    pub status: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AdminAddNoteRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub comment: String,
}

#[derive(Serialize)]
pub struct AdminAddNoteResponse {
    pub status: String,
    pub message: String,
}

// ==========================================
// Shared helpers
// ==========================================

type ApiError = (StatusCode, Json<String>);

/// Load a process by ID, verify user authorization, and load its template.
/// Shared across save-step, review, and upload handlers.
async fn load_authorized_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    process_id: Uuid,
    is_admin: bool,
) -> Result<(ProcessInfo, process::ProcessTemplate), ApiError> {
    let info = get_process_info(conn, process_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json("Process not found".to_string())))?;

    if !is_admin && info.user_id != user.user_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: You do not own this process".to_string()),
        ));
    }

    let template = process::ProcessTemplate::load_by_version(&info.template_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Template error: {}", e)),
        )
    })?;

    Ok((info, template))
}

fn find_step_name(template: &process::ProcessTemplate, step_id: i32) -> String {
    if template.step_id == step_id {
        return template.name.clone();
    }
    if let Some(ref subs) = template.substeps {
        for sub in subs {
            if let Some(name) = find_step_name_recursive(sub, step_id) {
                return name;
            }
        }
    }
    format!("Step {}", step_id)
}

fn find_step_name_recursive(step: &process::ProcessStepTemplate, step_id: i32) -> Option<String> {
    if step.step_id == step_id {
        return Some(step.name.clone());
    }
    if let Some(ref subs) = step.substeps {
        for sub in subs {
            if let Some(name) = find_step_name_recursive(sub, step_id) {
                return Some(name);
            }
        }
    }
    None
}

/// Build a `NotificationContext` for email dispatch.
/// Looks up the process owner's email from the DB. Returns `None` if
/// the lookup fails (notification will be skipped gracefully).
async fn build_notification_ctx(
    pool: &sqlx::PgPool,
    owner_user_id: i32,
    admin_email: &str,
    process_name: &str,
) -> Option<NotificationContext> {
    let mut conn = pool.acquire().await.ok()?;
    let user_email = get_user_email(&mut conn, owner_user_id)
        .await
        .ok()
        .flatten()?;
    Some(NotificationContext {
        user_email,
        admin_email: admin_email.to_string(),
        process_name: process_name.to_string(),
    })
}

/// For a resident user, resolve which ownership to use and find-or-create a process instance.
async fn resolve_or_init_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    template_id: &str,
    flat_id: Option<i32>,
) -> Result<(Uuid, String, Option<i32>), ApiError> {
    // Fetch active ownerships
    let active_ownerships = get_active_ownerships_for_user(conn, user.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    // Determine which ownership_id to use
    let selected_ownership = match flat_id {
        Some(fid) => {
            let oid = active_ownerships
                .iter()
                .find(|&&(_, flat)| flat == fid)
                .map(|&(oid, _)| oid)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json("No active ownership found for the specified flat_id".to_string()),
                    )
                })?;
            Some(oid)
        }
        None => {
            if active_ownerships.len() > 1 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json("Multiple active flats found. You must specify a flat_id".to_string()),
                ));
            } else if active_ownerships.is_empty() {
                None
            } else {
                Some(active_ownerships[0].0)
            }
        }
    };

    // Always initialize a new process if none was explicitly passed (as requested by user)
    let template = process::ProcessTemplate::load_by_version(template_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(format!("Template error: {}", e)),
        )
    })?;

    let process_id = process::db::init_process(conn, user.user_id, &template, selected_ownership)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(format!("Failed to initialize process: {}", e)),
            )
        })?;
    Ok((process_id, template_id.to_string(), selected_ownership))
}

/// Recursively map step templates to the response structure.
fn map_step_template(
    step_tpl: &process::ProcessStepTemplate,
    statuses: &HashMap<i32, process::state_machine::StepStatus>,
    field_history_map: &HashMap<i32, Vec<FieldHistoryEntry>>,
    latest_field_states: &HashMap<i32, process::state_machine::FieldState>,
    doc_title_map: &HashMap<Uuid, String>,
) -> StepResponse {
    let step_id = step_tpl.step_id;
    let status = statuses
        .get(&step_id)
        .copied()
        .unwrap_or(process::state_machine::StepStatus::Locked);

    let mut fields = Vec::new();
    if let Some(ref field_tpls) = step_tpl.fields {
        for f in field_tpls {
            let field_id = f.field_id;
            let latest = latest_field_states.get(&field_id);
            let history = field_history_map
                .get(&field_id)
                .cloned()
                .unwrap_or_default();

            let value = latest.map(|l| {
                let title = l.document_id.and_then(|id| doc_title_map.get(&id)).cloned();
                FieldValue {
                    text_value: l.text_value.clone(),
                    document_id: l.document_id,
                    document_title: title,
                }
            });

            fields.push(FieldResponse {
                template_field_id: field_id,
                name: f.name.clone(),
                r#type: f.r#type.clone(),
                filled_by: f.filled_by.clone(),
                is_optional: f.is_optional,
                value,
                history,
            });
        }
    }

    let mut substeps = Vec::new();
    if let Some(ref sub_tpls) = step_tpl.substeps {
        for sub in sub_tpls {
            substeps.push(map_step_template(
                sub,
                statuses,
                field_history_map,
                latest_field_states,
                doc_title_map,
            ));
        }
    }

    StepResponse {
        template_step_id: step_id,
        name: step_tpl.name.clone(),
        status,
        is_review_required: step_tpl.is_review_required,
        fields,
        substeps,
    }
}

/// Build a flat map of step_id -> step_name from the template tree.
fn build_step_name_map(template: &process::ProcessTemplate) -> HashMap<i32, String> {
    let mut names = HashMap::new();
    names.insert(template.step_id, template.name.clone());

    fn traverse(
        substeps: &Option<Vec<process::ProcessStepTemplate>>,
        names: &mut HashMap<i32, String>,
    ) {
        if let Some(ref subs) = substeps {
            for sub in subs {
                names.insert(sub.step_id, sub.name.clone());
                traverse(&sub.substeps, names);
            }
        }
    }
    traverse(&template.substeps, &mut names);
    names
}

// ==========================================
// Route handlers
// ==========================================

#[derive(Serialize)]
pub struct ProcessListResponse {
    pub processes: Vec<process::db::ProcessListItem>,
    pub flats: Vec<UserFlatResponse>,
}

/// POST /api/v1/process/list
pub async fn list_user_processes(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<ProcessListResponse>, ApiError> {
    let mut conn = state
        .pg_pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let is_admin = user.is_admin();
    let mut items = if is_admin {
        process::db::list_all_processes(&mut conn).await
    } else {
        list_processes_for_user(&mut conn, user.user_id).await
    }
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;
    // Evaluate accurate overall status for each process
    for item in items.iter_mut() {
        let mut resolved = false;
        if let Ok(template) = process::ProcessTemplate::load_by_version(&item.template_id) {
            if let Ok(turn) =
                process::db::get_process_turn(&mut conn, item.process_id, &template).await
            {
                item.status = Some(match turn {
                    process::state_machine::ProcessTurn::None => "COMPLETED".to_string(),
                    process::state_machine::ProcessTurn::Admin => {
                        "ADMIN_REVIEW_REQUIRED".to_string()
                    }
                    process::state_machine::ProcessTurn::User => "USER_INPUT_REQUIRED".to_string(),
                });
                resolved = true;
            }
        }

        if !resolved {
            // Fallback
            if item.steps_total > 0 && item.steps_completed == item.steps_total {
                item.status = Some("COMPLETED".to_string());
            } else {
                item.status = Some("USER_INPUT_REQUIRED".to_string());
            }
        }
    }

    // Load active flats for the user (empty for Admin if they don't have flats, but normal for user)
    let flats_db = get_user_active_flats(&mut conn, user.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;
    let flats = flats_db.into_iter().map(UserFlatResponse::from).collect();

    Ok(Json(ProcessListResponse {
        processes: items,
        flats,
    }))
}

/// POST /api/v1/process/state
pub async fn get_process_state(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessStateRequest>,
) -> Result<Json<ProcessStateResponse>, ApiError> {
    let mut tx = state
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let is_admin = user.is_admin();

    // --- Resolve process_id, template_id, ownership_id ---
    let (process_id, template_id, ownership_id) = match payload.process_id {
        Some(pid) => {
            let (info, _) = load_authorized_process(&mut tx, &user, pid, is_admin).await?;
            (pid, info.template_id, info.ownership_id)
        }
        None => {
            if is_admin {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json("process_id is required for administrative users".to_string()),
                ));
            }
            let target_template_id = payload.template_id.clone().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json("Either process_id or template_id must be provided".to_string()),
                )
            })?;
            resolve_or_init_process(&mut tx, &user, &target_template_id, payload.flat_id).await?
        }
    };

    // --- Load template and workflow ---
    let template = process::ProcessTemplate::load_by_version(&template_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Template error: {}", e)),
        )
    })?;

    let proc_state = process::db::load_state(&mut tx, process_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;
    let statuses = process::state_machine::compute_statuses(&template, &proc_state);

    // --- Fetch documents (titles only), reviews, field history ---
    let docs = crate::document::DocumentManager::get_documents_for_process(&mut *tx, process_id)
        .await
        .unwrap_or_default();

    let doc_title_map: HashMap<Uuid, String> = docs
        .into_iter()
        .map(|doc| (doc.document_id, doc.title))
        .collect();

    let reviews = get_process_reviews(&mut tx, process_id)
        .await
        .unwrap_or_default();

    let pf_rows = get_field_history(&mut tx, process_id)
        .await
        .unwrap_or_default();

    let mut field_history_map: HashMap<i32, Vec<FieldHistoryEntry>> = HashMap::new();
    for row in pf_rows {
        let title = row
            .document_id
            .and_then(|id| doc_title_map.get(&id))
            .cloned();

        field_history_map
            .entry(row.template_field_id)
            .or_default()
            .push(FieldHistoryEntry {
                text_value: row.text_value,
                document_id: row.document_id,
                document_title: title,
                updated_at: row.updated_at,
            });
    }

    // Latest field states (first entry per field since ordered DESC)
    let mut latest_field_states = HashMap::new();
    for (fid, hist) in &field_history_map {
        if let Some(latest_entry) = hist.first() {
            latest_field_states.insert(
                *fid,
                process::state_machine::FieldState {
                    text_value: latest_entry.text_value.clone(),
                    document_id: latest_entry.document_id,
                },
            );
        }
    }

    // --- Compute next actions ---
    let step_info_map = process::state_machine::build_step_info_map(&template);
    let step_names = build_step_name_map(&template);
    let mut next_actions = Vec::new();

    for (&step_id, &status) in &statuses {
        if let Some(info) = step_info_map.get(&step_id) {
            if info.fields.is_empty() {
                continue;
            }
            match status {
                process::state_machine::StepStatus::PendingReview => {
                    next_actions.push(NextActionInfo {
                        template_step_id: step_id,
                        name: step_names.get(&step_id).cloned().unwrap_or_default(),
                        assigned_to: "admin".to_string(),
                    });
                }
                process::state_machine::StepStatus::Open => {
                    let has_admin = info.fields.iter().any(|f| f.filled_by == "admin");
                    let has_user = info.fields.iter().any(|f| f.filled_by == "user");
                    if has_user {
                        next_actions.push(NextActionInfo {
                            template_step_id: step_id,
                            name: step_names.get(&step_id).cloned().unwrap_or_default(),
                            assigned_to: "user".to_string(),
                        });
                    }
                    if has_admin {
                        next_actions.push(NextActionInfo {
                            template_step_id: step_id,
                            name: step_names.get(&step_id).cloned().unwrap_or_default(),
                            assigned_to: "admin".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // --- Build response tree ---
    let mut steps_res = Vec::new();
    if let Some(ref top_steps) = template.substeps {
        for top_step in top_steps {
            steps_res.push(map_step_template(
                top_step,
                &statuses,
                &field_history_map,
                &latest_field_states,
                &doc_title_map,
            ));
        }
    }

    let flat_label: Option<String> = if let Some(oid) = ownership_id {
        get_flat_label_for_ownership(&mut tx, oid)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?
    } else {
        None
    };

    let is_completed =
        statuses.get(&template.step_id) == Some(&process::state_machine::StepStatus::Completed);

    // --- Fetch admin-only comments (only for admin callers) ---
    let admin_comments = if is_admin {
        get_admin_comments(&mut tx, process_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Commit transaction — needed for the auto-init path which may create a new process
    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    Ok(Json(ProcessStateResponse {
        process_id,
        template_id: template.template_id.clone(),
        process_name: template.name.clone(),
        ownership_id,
        flat_label,
        is_completed,
        next_actions,
        reviews,
        admin_comments,
        steps: steps_res,
    }))
}

/// POST /api/v1/process/save-step
pub async fn save_process_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SaveStepRequest>,
) -> Result<Json<SaveStepResponse>, ApiError> {
    let mut tx = state
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let is_admin = user.is_admin();
    let (info, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, is_admin).await?;

    // Validate document ownership and upload confirmation
    for f in &payload.fields {
        if let Some(doc_id) = f.document_id {
            // Check if document is valid AND the user has permission to access it
            let doc_opt = state
                .document_manager
                .get_document(doc_id, &user)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

            match doc_opt {
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(format!(
                            "Document ID {} is invalid or you do not have permission to access it",
                            doc_id
                        )),
                    ));
                }
                Some(ref doc) if !doc.document.is_upload_confirmed => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(format!(
                            "Document ID {} has not been uploaded yet. Please complete the upload first.",
                            doc_id
                        )),
                    ));
                }
                // Process fields only ever hold flat-scoped documents. Other categories
                // (PUBLIC, COMMITTEE, FINANCE, GOVERNANCE, OPERATIONAL) are visible in a
                // user's document list but must never be attachable to a process step.
                Some(ref doc)
                    if doc.document.category
                        != crate::document::DocumentCategory::ResidentSpecific =>
                {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(format!(
                            "Document ID {} is not a Resident Specific document and cannot be attached to a process step",
                            doc_id
                        )),
                    ));
                }
                _ => {}
            }
        }
    }

    let fields_submitted: Vec<process::state_machine::SaveFieldInput> = payload
        .fields
        .into_iter()
        .map(|f| process::state_machine::SaveFieldInput {
            template_field_id: f.template_field_id,
            text_value: f.text_value,
            document_id: f.document_id,
        })
        .collect();

    let step_status = process::db::save_step(
        &mut tx,
        payload.process_id,
        &template,
        payload.template_step_id,
        &fields_submitted,
        is_admin,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let is_completed = step_status == process::state_machine::StepStatus::Completed;

    // Commit transaction — all field inserts + step updates are atomic
    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    // Only email on meaningful cross-role transitions (user→admin review needed)
    if step_status == process::state_machine::StepStatus::PendingReview {
        let step_name = find_step_name(&template, payload.template_step_id);
        if let Some(ctx) = build_notification_ctx(
            &state.pg_pool,
            info.user_id,
            &state.super_admin_email,
            &template.name,
        )
        .await
        {
            process::notification::notify_admin_pending_review(
                &ctx,
                payload.process_id,
                &step_name,
            )
            .await;
        }
    }

    Ok(Json(SaveStepResponse {
        status: "success".to_string(),
        message: "Step saved successfully".to_string(),
        step: SavedStepInfo {
            template_step_id: payload.template_step_id,
            is_completed,
        },
    }))
}

/// POST /api/v1/admin/process/review
pub async fn admin_review_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminReviewRequest>,
) -> Result<Json<AdminReviewResponse>, ApiError> {
    let mut tx = state
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let (info, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, true).await?;

    let flagged = payload.flagged_step_ids.as_deref().unwrap_or(&[]);
    let is_completed = process::db::review_step(
        &mut tx,
        payload.process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: payload.template_step_id,
            reviewer_user_id: Some(user.user_id),
            is_approved: payload.is_approved,
            notes: payload.notes.as_deref(),
            flagged_step_ids: flagged,
        },
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let message = if payload.is_approved {
        "Review recorded successfully. Step is marked as completed.".to_string()
    } else {
        "Review recorded successfully. Step is marked as incomplete.".to_string()
    };

    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let step_name = find_step_name(&template, payload.template_step_id);
    if let Some(ctx) = build_notification_ctx(
        &state.pg_pool,
        info.user_id,
        &state.super_admin_email,
        &template.name,
    )
    .await
    {
        // Resolve flagged step IDs to human-readable names for the email
        let flagged_names: Vec<String> = payload
            .flagged_step_ids
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|&id| find_step_name(&template, id))
            .collect();

        process::notification::notify_user_review(
            &ctx,
            process::notification::ReviewNotificationParams {
                process_id: payload.process_id,
                step_name: &step_name,
                is_approved: payload.is_approved,
                notes: payload.notes.as_deref(),
                flagged_step_names: &flagged_names,
            },
        )
        .await;
    }

    Ok(Json(AdminReviewResponse {
        status: "success".to_string(),
        message,
        is_completed,
    }))
}

/// POST /api/v1/admin/process/revoke
pub async fn admin_revoke_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminRevokeRequest>,
) -> Result<Json<AdminRevokeResponse>, ApiError> {
    let mut tx = state
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let (info, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, true).await?;

    process::db::reopen_step(
        &mut tx,
        payload.process_id,
        &template,
        payload.template_step_id,
        user.user_id,
        payload.notes.as_deref(),
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;
    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let step_name = find_step_name(&template, payload.template_step_id);
    if let Some(ctx) = build_notification_ctx(
        &state.pg_pool,
        info.user_id,
        &state.super_admin_email,
        &template.name,
    )
    .await
    {
        process::notification::notify_user_revoke(
            &ctx,
            payload.process_id,
            &step_name,
            payload.notes.as_deref(),
        )
        .await;
    }

    Ok(Json(AdminRevokeResponse {
        status: "success".to_string(),
        message: "Step revoked successfully".to_string(),
    }))
}

#[derive(Deserialize)]
pub struct ProcessAdminUploadUrlRequest {
    pub process_id: Uuid,
    pub title: String,
    pub filename: String,
    pub content_type: Option<String>,
}

#[derive(Serialize)]
pub struct ProcessAdminUploadUrlResponse {
    pub document_id: String,
    pub upload_url: String,
}

/// POST /api/v1/admin/process/upload-url
/// Creates an upload intent for a process document uploaded by an admin.
/// The backend securely derives the flat and forces RESIDENT_SPECIFIC (private) semantics.
pub async fn process_admin_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessAdminUploadUrlRequest>,
) -> Result<(StatusCode, Json<ProcessAdminUploadUrlResponse>), ApiError> {
    let mut conn = state
        .pg_pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let (info, _) = load_authorized_process(&mut conn, &user, payload.process_id, true).await?;

    let mut flat_id: Option<i32> = None;
    if let Some(oid) = info.ownership_id {
        flat_id = get_flat_id_for_ownership(&mut conn, oid)
            .await
            .unwrap_or(None);
    }

    // Validate file extension
    let ext = payload
        .filename
        .split('.')
        .next_back()
        .unwrap_or("")
        .to_lowercase();
    if !crate::handlers::documents::VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(format!(
                "Invalid file type '{}'. Allowed: {:?}",
                ext,
                crate::handlers::documents::VALID_UPLOAD_EXTENSIONS
            )),
        ));
    }

    let content_type = crate::handlers::documents::derive_content_type_from_ext(ext.as_str());

    // flat_id is already an Option<i32>

    use crate::document::DocumentCategory;

    let (result, upload_url, _key) = state
        .document_manager
        .admin_create_upload_intent_by_id(
            crate::document::AdminUploadIntentInput {
                title: &payload.title,
                description: None,
                category: DocumentCategory::ResidentSpecific,
                metadata: None,
                content_type: &content_type,
                uploaded_by: Some(user.user_id),
            },
            flat_id,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(ProcessAdminUploadUrlResponse {
            document_id: result.document.document_id.to_string(),
            upload_url,
        }),
    ))
}

/// POST /api/v1/process/download-url
/// On-demand presigned download URL for a process document.
pub async fn process_download_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<DocumentUrlRequest>,
) -> Result<Json<DocumentUrlResponse>, ApiError> {
    let is_admin = user.is_admin();

    // Auth check
    {
        let mut conn = state
            .pg_pool
            .acquire()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;
        load_authorized_process(&mut conn, &user, payload.process_id, is_admin).await?;
    }

    // Fetch the document using the centralized document manager which enforces authorization
    let doc = state
        .document_manager
        .get_document(payload.document_id, &user)
        .await
        .map_err(|e| {
            if e == "FORBIDDEN" {
                (StatusCode::FORBIDDEN, Json(e))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(e))
            }
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json("Document not found".to_string()),
            )
        })?;

    let url = doc.download_url.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json("Failed to generate download URL".to_string()),
        )
    })?;

    Ok(Json(DocumentUrlResponse { download_url: url }))
}

#[derive(Serialize)]
pub struct UserFlatResponse {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
}

impl From<UserFlatInfo> for UserFlatResponse {
    fn from(f: UserFlatInfo) -> Self {
        Self {
            flat_id: f.flat_id,
            wing: f.wing,
            flat_number: f.flat_number,
        }
    }
}

// ==========================================
// Admin: Add standalone internal note
// ==========================================

pub async fn admin_add_note(
    user: SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminAddNoteRequest>,
) -> Result<Json<AdminAddNoteResponse>, ApiError> {
    let comment = payload.comment.trim();
    if comment.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Comment cannot be empty".to_string()),
        ));
    }

    let mut conn = state
        .pg_pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    // Verify the process exists and admin has access
    load_authorized_process(&mut conn, &user.0, payload.process_id, true).await?;

    process::db::insert_admin_comment(
        &mut conn,
        payload.process_id,
        payload.template_step_id,
        user.0.user_id,
        comment,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    Ok(Json(AdminAddNoteResponse {
        status: "success".to_string(),
        message: "Internal note added".to_string(),
    }))
}
