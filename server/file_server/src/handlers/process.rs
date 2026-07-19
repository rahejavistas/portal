use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::db::{
    get_active_ownerships_for_user, get_admin_comments, get_field_history,
    get_flat_info_for_ownership, get_process_info, get_process_reviews, get_user_active_flats,
    get_user_email, list_processes_for_user, AdminComment, ProcessInfo,
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
    /// Set by the plain /process page: forces "just my own process" behavior even for
    /// a caller who also holds process.reviewer/process.view, so which page you're on
    /// fully determines what you see — no client-side permission branching needed.
    #[serde(default)]
    pub personal_only: bool,
}

#[derive(Deserialize, Default)]
pub struct ProcessListRequest {
    #[serde(default)]
    pub personal_only: bool,
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

/// Shorthand for the extremely common "DB/internal error -> 500" mapping,
/// cutting the repeated `.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))`
/// closure down to a single call.
trait ResultExt<T> {
    fn internal_err(self) -> Result<T, ApiError>;
}

impl<T, E: ToString> ResultExt<T> for Result<T, E> {
    fn internal_err(self) -> Result<T, ApiError> {
        self.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))
    }
}

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
        .internal_err()?
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

/// Look up a step's display name from the template. Falls back to a generic
/// label if the ID isn't in the template (shouldn't happen in practice).
fn find_step_name(template: &process::ProcessTemplate, step_id: i32) -> String {
    process::state_machine::build_step_info_map(template)
        .get(&step_id)
        .map(|info| info.name.clone())
        .unwrap_or_else(|| format!("Step {}", step_id))
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
    let active_ownerships = get_active_ownerships_for_user(conn, user.user_id)
        .await
        .internal_err()?;

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
    Json(payload): Json<ProcessListRequest>,
) -> Result<Json<ProcessListResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;

    // Read-only oversight (process.view) sees every resident's process list, same as a
    // reviewer, but never gains the write bypass reviewers get further down the flow.
    // personal_only (set by the plain /process page) always forces the caller's own view.
    let can_view_all = !payload.personal_only
        && (user.has_permission(authentication::Permission::ProcessReviewer)
            || user.has_permission(authentication::Permission::ProcessView));
    let mut items = if can_view_all {
        process::db::list_all_processes(&mut conn).await
    } else {
        list_processes_for_user(&mut conn, user.user_id).await
    }
    .internal_err()?;
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
        .internal_err()?;
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
    let mut tx = state.pg_pool.begin().await.internal_err()?;

    // Read-only oversight (process.view) sees any resident's process state, same as a
    // reviewer — this handler never writes to another user's process, only the caller's own.
    // personal_only (set by the plain /process page) always forces the caller's own view.
    let can_view_all = !payload.personal_only
        && (user.has_permission(authentication::Permission::ProcessReviewer)
            || user.has_permission(authentication::Permission::ProcessView));

    // --- Resolve process_id, template_id, ownership_id ---
    let (process_id, template_id, ownership_id) = match payload.process_id {
        Some(pid) => {
            let (info, _) = load_authorized_process(&mut tx, &user, pid, can_view_all).await?;
            (pid, info.template_id, info.ownership_id)
        }
        None => {
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

    let steps = process::db::load_steps(&mut tx, process_id)
        .await
        .internal_err()?;

    // --- Fetch documents (titles only), full review history, full field history ---
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

    // Latest field state per field (history is ordered DESC, so first = latest).
    let latest_field_states: HashMap<i32, process::state_machine::FieldState> = field_history_map
        .iter()
        .filter_map(|(&fid, hist)| {
            hist.first().map(|latest| {
                (
                    fid,
                    process::state_machine::FieldState {
                        text_value: latest.text_value.clone(),
                        document_id: latest.document_id,
                    },
                )
            })
        })
        .collect();

    // Review state per step for the state machine, derived from the same full
    // history fetched above for display — avoids a second query against
    // process_step_reviews just to get (is_approved, created_at) pairs.
    let mut reviews_by_step: HashMap<i32, Vec<process::state_machine::ReviewState>> =
        HashMap::new();
    for r in &reviews {
        reviews_by_step.entry(r.template_step_id).or_default().push(
            process::state_machine::ReviewState {
                is_approved: r.is_approved,
                created_at: r.created_at,
            },
        );
    }

    let proc_state = process::state_machine::ProcessStateInput {
        steps,
        fields: latest_field_states,
        reviews: reviews_by_step,
    };
    let statuses = process::state_machine::compute_statuses(&template, &proc_state);

    // --- Compute next actions ---
    let step_info_map = process::state_machine::build_step_info_map(&template);
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
                        name: info.name.clone(),
                        assigned_to: "admin".to_string(),
                    });
                }
                process::state_machine::StepStatus::Open => {
                    let has_admin = info.fields.iter().any(|f| f.filled_by == "admin");
                    let has_user = info.fields.iter().any(|f| f.filled_by == "user");
                    if has_user {
                        next_actions.push(NextActionInfo {
                            template_step_id: step_id,
                            name: info.name.clone(),
                            assigned_to: "user".to_string(),
                        });
                    }
                    if has_admin {
                        next_actions.push(NextActionInfo {
                            template_step_id: step_id,
                            name: info.name.clone(),
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
                &proc_state.fields,
                &doc_title_map,
            ));
        }
    }

    let flat_label: Option<String> = if let Some(oid) = ownership_id {
        get_flat_info_for_ownership(&mut tx, oid)
            .await
            .internal_err()?
            .map(|f| f.label)
    } else {
        None
    };

    let is_completed =
        statuses.get(&template.step_id) == Some(&process::state_machine::StepStatus::Completed);

    // --- Fetch admin-only comments (visible to anyone with oversight access) ---
    let admin_comments = if can_view_all {
        get_admin_comments(&mut tx, process_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Commit transaction — needed for the auto-init path which may create a new process
    tx.commit().await.internal_err()?;

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
    let mut tx = state.pg_pool.begin().await.internal_err()?;

    // Write path: deliberately NOT widened to process.view — that permission is read-only
    // oversight and must never gain the ability to save a step on someone else's process.
    let is_admin = user.has_permission(authentication::Permission::ProcessReviewer);
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
                .internal_err()?;

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
                // are visible in a user's document list but must never be attachable to a
                // process step.
                Some(ref doc)
                    if doc.document.category != crate::document::RESIDENT_SPECIFIC_CATEGORY =>
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
    tx.commit().await.internal_err()?;

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
    let mut tx = state.pg_pool.begin().await.internal_err()?;

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

    tx.commit().await.internal_err()?;

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
    let mut tx = state.pg_pool.begin().await.internal_err()?;

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
    tx.commit().await.internal_err()?;

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
    let mut conn = state.pg_pool.acquire().await.internal_err()?;

    let (info, _) = load_authorized_process(&mut conn, &user, payload.process_id, true).await?;

    let mut flat_id: Option<i32> = None;
    if let Some(oid) = info.ownership_id {
        flat_id = get_flat_info_for_ownership(&mut conn, oid)
            .await
            .unwrap_or(None)
            .map(|f| f.flat_id);
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

    let (result, upload_url, _key) = state
        .document_manager
        .admin_create_upload_intent_by_id(
            crate::document::AdminUploadIntentInput {
                title: &payload.title,
                description: None,
                category: crate::document::RESIDENT_SPECIFIC_CATEGORY,
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
    // Read-only oversight (process.view) can download documents attached to any process,
    // same as a reviewer — this handler never writes anything.
    let can_view_all = user.has_permission(authentication::Permission::ProcessReviewer)
        || user.has_permission(authentication::Permission::ProcessView);

    // Auth check: is the caller allowed to view this process at all?
    let owner_id = {
        let mut conn = state.pg_pool.acquire().await.internal_err()?;
        let (info, _) =
            load_authorized_process(&mut conn, &user, payload.process_id, can_view_all).await?;
        info.user_id
    };

    // Documents aren't necessarily attached to a process step yet when this is called
    // (e.g. previewing an entry from the resident's own library before submitting it),
    // so document access is evaluated as the process OWNER would see it, not the caller —
    // being cleared to view/review this process authorizes whatever its owner can see,
    // regardless of the caller's own unrelated category/flat grants.
    let owner = state
        .permissions_manager
        .get_user_by_id(owner_id)
        .await
        .internal_err()?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json("Process owner not found".to_string()),
            )
        })?;

    let doc = state
        .document_manager
        .get_document(payload.document_id, &owner)
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

    let mut conn = state.pg_pool.acquire().await.internal_err()?;

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
    .internal_err()?;

    Ok(Json(AdminAddNoteResponse {
        status: "success".to_string(),
        message: "Internal note added".to_string(),
    }))
}
