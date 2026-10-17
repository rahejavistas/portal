//! Read-only process views: the process list and the full state tree for one process.

use super::{build_actor_context, load_authorized_process, ApiError, ResultExt};
use crate::extractors::SessionUser;
use crate::handlers::accounting::pdf_response;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::db::{
    get_active_ownerships_for_user, get_admin_comments, get_field_history,
    get_flat_info_for_ownership, get_process_reviews, get_user_active_flats,
    list_processes_for_user, AdminComment, ProcessReviewHistory as ReviewHistoryInfo, UserFlatInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ProcessStateRequest {
    pub process_id: Option<Uuid>,
    /// For starting a new process: which template to start (always its active version).
    pub template_id: Option<i32>,
    pub flat_id: Option<i32>,
    /// Set by the plain /process page: forces "just my own process" even for a
    /// process.reviewer/process.view caller.
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
    pub actor_email: Option<String>,
    pub acted_as_designation: Option<String>,
}

#[derive(Serialize)]
pub struct FieldValue {
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub document_title: Option<String>,
    pub document_content_type: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct FieldSignatureStatus {
    pub requirement_id: i32,
    pub signer_name: String,
    pub signer_user_id: i32,
    pub sign_order: i32,
    pub status: String,
}

#[derive(Serialize)]
pub struct FieldResponse {
    pub template_field_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub r#type: process::FieldType,
    pub is_optional: bool,
    /// Blocks completion until a reviewer attaches and every signer signs.
    pub requires_signature: bool,
    /// Designations the "Add Signers" picker scopes to (empty = no restriction).
    pub required_signs_from_designations: Vec<String>,
    /// `Enum` fields only — the closed list of valid values.
    pub options: Option<Vec<String>>,
    pub example_document_id: Option<Uuid>,
    pub example_document_title: Option<String>,
    pub value: Option<FieldValue>,
    pub history: Vec<FieldHistoryEntry>,
    /// Empty until a reviewer attaches signers via the process review screen.
    pub signature_status: Vec<FieldSignatureStatus>,
}

#[derive(Serialize)]
pub struct StepResponse {
    pub template_step_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: process::state_machine::StepStatus,
    pub filled_by: process::StepOwner,
    /// `Designation`-owned steps only — the human label for `filled_by`'s code.
    pub filled_by_designation_label: Option<String>,
    pub fields: Vec<FieldResponse>,
    pub substeps: Vec<StepResponse>,
}

#[derive(Serialize)]
pub struct ProcessStateResponse {
    pub process_id: Uuid,
    pub inward_reg_no: String,
    pub template_version_id: i32,
    pub process_name: String,
    pub ownership_id: Option<i32>,
    pub flat_label: Option<String>,
    pub is_completed: bool,
    pub next_actions: Vec<NextActionInfo>,
    pub reviews: Vec<ReviewHistoryInfo>,
    /// Standalone admin-only comments. Only populated for admin callers;
    /// empty for residents.
    pub admin_comments: Vec<AdminComment>,
    /// Append-only status/verification/return trail. Only populated for admin
    /// callers, same visibility as `admin_comments`.
    pub audit_log: Vec<process::audit::AuditLogEntry>,
    pub effective_due_date: Option<chrono::NaiveDate>,
    pub is_sla_paused: bool,
    /// Real working days from SLA start to closure, net of pause time. `None` until
    /// the process has actually closed (or the template has no SLA tracking).
    pub tat_actual_working_days: Option<i32>,
    pub steps: Vec<StepResponse>,
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

#[derive(Serialize)]
pub struct ProcessListItemView {
    #[serde(flatten)]
    pub item: process::db::ProcessListItem,
    /// Calendar days since creation, frozen at completion instead of growing forever.
    pub days_running: i64,
    /// `sla_due_date` shifted forward by time spent paused for a return. `None` until
    /// the checklist is fully submitted (or the template has no `tat_working_days`).
    pub effective_due_date: Option<chrono::NaiveDate>,
    pub is_sla_paused: bool,
    pub is_overdue: bool,
}

fn build_list_item_view(
    item: process::db::ProcessListItem,
    holidays: &std::collections::HashSet<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> ProcessListItemView {
    let is_completed = item.current_turn.as_deref() == Some("none");
    let days_running_end = if is_completed {
        item.updated_at
    } else {
        chrono::Utc::now()
    };
    let days_running = (days_running_end - item.created_at).num_days();
    let effective_due_date = item.sla_due_date.map(|due| {
        process::working_days::add_working_days(due, item.sla_paused_working_days, holidays)
    });
    let is_overdue = effective_due_date
        .map(|due| !is_completed && today > due)
        .unwrap_or(false);
    let is_sla_paused = item.sla_paused_at.is_some();

    ProcessListItemView {
        item,
        days_running,
        effective_due_date,
        is_sla_paused,
        is_overdue,
    }
}

#[derive(Serialize)]
pub struct ProcessListResponse {
    pub processes: Vec<ProcessListItemView>,
    pub flats: Vec<UserFlatResponse>,
}

/// Whether `actor` can personally act on `item` right now. `template_cache` avoids
/// reloading the same version for every row sharing it.
async fn compute_pending_on_caller(
    conn: &mut sqlx::PgConnection,
    item: &process::db::ProcessListItem,
    actor: &process::state_machine::ActorContext,
    template_cache: &mut HashMap<i32, process::ProcessTemplate>,
) -> Option<bool> {
    let template = match template_cache.get(&item.template_version_id) {
        Some(t) => t.clone(),
        None => {
            let t = process::template_store::load_template_version(conn, item.template_version_id)
                .await
                .ok()?;
            template_cache.insert(item.template_version_id, t.clone());
            t
        }
    };
    let wing_rep_code = match item.ownership_id {
        Some(oid) => process::db::get_wing_representative_code_for_ownership(conn, oid)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let template =
        process::template_store::resolve_wing_representatives(template, wing_rep_code.as_deref())
            .ok()?;

    let steps = process::db::load_steps(conn, item.process_id).await.ok()?;
    let reviews = get_process_reviews(conn, item.process_id)
        .await
        .unwrap_or_default();
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
    let state_input = process::state_machine::ProcessStateInput {
        steps,
        reviews: reviews_by_step,
        ..Default::default()
    };
    Some(process::state_machine::is_pending_on_actor(
        &template,
        &state_input,
        actor,
    ))
}

/// Resolves ownership and find-or-creates a process against template_id's active version.
async fn resolve_or_init_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    template_id: i32,
    flat_id: Option<i32>,
) -> Result<(Uuid, i32, Option<i32>, process::ProcessTemplate, Vec<i32>), ApiError> {
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

    let version_id = process::template_store::get_active_version_id(conn, template_id)
        .await
        .internal_err()?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(format!("No active version for template {}", template_id)),
            )
        })?;

    let template = process::template_store::load_template_version(conn, version_id)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(format!("Template error: {}", e)),
            )
        })?;
    let wing_rep_code = match selected_ownership {
        Some(oid) => process::db::get_wing_representative_code_for_ownership(conn, oid)
            .await
            .internal_err()?,
        None => None,
    };
    let template =
        process::template_store::resolve_wing_representatives(template, wing_rep_code.as_deref())
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let init_outcome = process::db::init_process(
        conn,
        user.user_id,
        version_id,
        &template,
        selected_ownership,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to initialize process: {}", e)),
        )
    })?;
    Ok((
        init_outcome.process_id,
        version_id,
        selected_ownership,
        template,
        init_outcome.newly_opened_step_ids,
    ))
}

fn collect_example_document_ids(
    fields: &Option<Vec<process::ProcessFieldTemplate>>,
    substeps: &Option<Vec<process::ProcessStepTemplate>>,
    out: &mut Vec<Uuid>,
) {
    if let Some(fs) = fields {
        out.extend(fs.iter().filter_map(|f| f.example_document_id));
    }
    if let Some(subs) = substeps {
        for s in subs {
            collect_example_document_ids(&s.fields, &s.substeps, out);
        }
    }
}

fn collect_designation_codes(
    substeps: &Option<Vec<process::ProcessStepTemplate>>,
    out: &mut Vec<String>,
) {
    let Some(subs) = substeps else { return };
    for s in subs {
        if let process::StepOwner::Designation { code } = &s.filled_by {
            out.push(code.clone());
        }
        collect_designation_codes(&s.substeps, out);
    }
}

/// Read-only lookup maps threaded through `map_step_template`'s recursion.
struct StepMapContext<'a> {
    statuses: &'a HashMap<i32, process::state_machine::StepStatus>,
    field_history_map: &'a HashMap<i32, Vec<FieldHistoryEntry>>,
    latest_field_states: &'a HashMap<i32, process::state_machine::FieldState>,
    doc_title_map: &'a HashMap<Uuid, (String, Option<String>)>,
    example_doc_title_map: &'a HashMap<Uuid, String>,
    signature_map: &'a HashMap<Uuid, Vec<FieldSignatureStatus>>,
    designation_label_map: &'a HashMap<String, String>,
}

/// Recursively map step templates to the response structure.
fn map_step_template(
    step_tpl: &process::ProcessStepTemplate,
    ctx: &StepMapContext,
) -> StepResponse {
    let step_id = step_tpl.step_id;
    let status = ctx
        .statuses
        .get(&step_id)
        .copied()
        .unwrap_or(process::state_machine::StepStatus::Locked);

    let mut fields = Vec::new();
    if let Some(ref field_tpls) = step_tpl.fields {
        for f in field_tpls {
            let field_id = f.field_id;
            let latest = ctx.latest_field_states.get(&field_id);
            let history = ctx
                .field_history_map
                .get(&field_id)
                .cloned()
                .unwrap_or_default();

            let value = latest.map(|l| {
                let (title, content_type) = l
                    .document_id
                    .and_then(|id| ctx.doc_title_map.get(&id))
                    .cloned()
                    .map(|(t, c)| (Some(t), c))
                    .unwrap_or((None, None));
                FieldValue {
                    text_value: l.text_value.clone(),
                    document_id: l.document_id,
                    document_title: title,
                    document_content_type: content_type,
                }
            });

            let signature_status = latest
                .and_then(|l| l.document_id)
                .and_then(|id| ctx.signature_map.get(&id))
                .cloned()
                .unwrap_or_default();

            fields.push(FieldResponse {
                template_field_id: field_id,
                name: f.name.clone(),
                description: f.description.clone(),
                r#type: f.r#type,
                is_optional: f.is_optional,
                requires_signature: f.requires_signature,
                required_signs_from_designations: f.required_signs_from_designations.clone(),
                options: f.options.clone(),
                example_document_id: f.example_document_id,
                example_document_title: f
                    .example_document_id
                    .and_then(|id| ctx.example_doc_title_map.get(&id))
                    .cloned(),
                value,
                history,
                signature_status,
            });
        }
    }

    let mut substeps = Vec::new();
    if let Some(ref sub_tpls) = step_tpl.substeps {
        for sub in sub_tpls {
            substeps.push(map_step_template(sub, ctx));
        }
    }

    let filled_by_designation_label = match &step_tpl.filled_by {
        process::StepOwner::User | process::StepOwner::Admin => None,
        process::StepOwner::Designation { code } => ctx.designation_label_map.get(code).cloned(),
        process::StepOwner::WingRepresentative => unreachable!(
            "WingRepresentative is resolved into a concrete Designation before reaching here"
        ),
    };

    StepResponse {
        template_step_id: step_id,
        name: step_tpl.name.clone(),
        description: step_tpl.description.clone(),
        status,
        filled_by: step_tpl.filled_by.clone(),
        filled_by_designation_label,
        fields,
        substeps,
    }
}

/// POST /api/v1/process/list
pub async fn list_user_processes(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessListRequest>,
) -> Result<Json<ProcessListResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;

    let actor = build_actor_context(&state.permissions_manager, &user).await;
    // process.view sees every resident's list too, but never gains write access.
    let is_global_reviewer = super::can_view_all_processes(&user);
    // A plain designation holder (no global permission) gets the reviewer-style list
    // too, but it's filtered below to only what's actually pending on their own
    // designation — never the full, unscoped resident list.
    let is_scoped_reviewer = !is_global_reviewer && !actor.designation_codes.is_empty();
    let can_view_all = !payload.personal_only && (is_global_reviewer || is_scoped_reviewer);
    let mut items = if can_view_all {
        process::db::list_all_processes(&mut conn).await
    } else {
        list_processes_for_user(&mut conn, user.user_id).await
    }
    .internal_err()?;
    for item in items.iter_mut() {
        item.status = Some(match item.current_turn.as_deref() {
            Some("none") => "COMPLETED".to_string(),
            Some("admin") => "ADMIN_REVIEW_REQUIRED".to_string(),
            Some("user") => "USER_INPUT_REQUIRED".to_string(),
            // Predates the current_turn column — falls back until its next write.
            _ if item.steps_total > 0 && item.steps_completed == item.steps_total => {
                "COMPLETED".to_string()
            }
            _ => "USER_INPUT_REQUIRED".to_string(),
        });
    }

    if can_view_all {
        let mut template_cache: HashMap<i32, process::ProcessTemplate> = HashMap::new();
        for item in items.iter_mut() {
            if item.status.as_deref() == Some("COMPLETED") {
                item.pending_on_caller = Some(false);
                continue;
            }
            item.pending_on_caller =
                compute_pending_on_caller(&mut conn, item, &actor, &mut template_cache).await;
        }
        if is_scoped_reviewer {
            items.retain(|item| item.pending_on_caller == Some(true));
        }
    }

    // Load active flats for the user (empty for Admin if they don't have flats, but normal for user)
    let flats_db = get_user_active_flats(&mut conn, user.user_id)
        .await
        .internal_err()?;
    let flats = flats_db.into_iter().map(UserFlatResponse::from).collect();

    let holidays = process::db::get_holidays(&mut conn).await.internal_err()?;
    let today = chrono::Utc::now().date_naive();
    let processes = items
        .into_iter()
        .map(|item| build_list_item_view(item, &holidays, today))
        .collect();

    Ok(Json(ProcessListResponse { processes, flats }))
}

/// Resolves an existing `process_id` (authorization checked) or fetch-or-creates one
/// from a `template_id`. Returns the already-loaded template too, for reuse.
async fn resolve_process_target(
    tx: &mut sqlx::PgConnection,
    state: &AppState,
    user: &authentication::User,
    payload: &ProcessStateRequest,
    can_view_all: bool,
    designation_codes: &std::collections::HashSet<String>,
) -> Result<(Uuid, i32, Option<i32>, process::ProcessTemplate, Vec<i32>), ApiError> {
    match payload.process_id {
        Some(pid) => {
            // A signer on this process gets read-only access to it even without
            // process.reviewer/process.view.
            let is_authorized = can_view_all
                || state
                    .document_manager
                    .signer_has_requirement_for_process(user.user_id, pid)
                    .await
                    .internal_err()?;
            let (info, template) =
                load_authorized_process(tx, user, pid, is_authorized, designation_codes).await?;
            // An already-existing process has no "newly opened" steps from this call.
            Ok((
                pid,
                info.template_version_id,
                info.ownership_id,
                template,
                vec![],
            ))
        }
        None => {
            let target_template_id = payload.template_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json("Either process_id or template_id must be provided".to_string()),
                )
            })?;
            resolve_or_init_process(tx, user, target_template_id, payload.flat_id).await
        }
    }
}

/// Document IDs currently blocking their field from counting as filled, plus
/// per-document signer progress for the response.
async fn compute_signature_blocking_state(
    tx: &mut sqlx::PgConnection,
    template: &process::ProcessTemplate,
    latest_field_states: &HashMap<i32, process::state_machine::FieldState>,
) -> Result<
    (
        std::collections::HashSet<Uuid>,
        HashMap<Uuid, Vec<FieldSignatureStatus>>,
    ),
    ApiError,
> {
    let field_doc_ids: Vec<Uuid> = latest_field_states
        .values()
        .filter_map(|f| f.document_id)
        .collect();

    let (mut blocking_document_ids, signature_map) = if field_doc_ids.is_empty() {
        (std::collections::HashSet::new(), HashMap::new())
    } else {
        // Requirements are keyed to the original document_id, but a fully-signed field
        // gets repointed at signed_document_id — match on either.
        type SigRow = (Uuid, Option<Uuid>, i32, String, i32, i32, String);
        let sig_rows: Vec<SigRow> = sqlx::query_as(
            "SELECT r.document_id, r.signed_document_id, r.requirement_id, s.name, s.user_id, r.sign_order, r.status::TEXT \
             FROM document_signature_requirements r \
             JOIN document_signers s ON s.signer_id = r.signer_id \
             WHERE r.document_id = ANY($1) OR r.signed_document_id = ANY($1)",
        )
        .bind(&field_doc_ids)
        .fetch_all(&mut *tx)
        .await
        .internal_err()?;

        let mut blocking = std::collections::HashSet::new();
        let mut sig_map: HashMap<Uuid, Vec<FieldSignatureStatus>> = HashMap::new();
        for (
            doc_id,
            signed_document_id,
            requirement_id,
            signer_name,
            signer_user_id,
            sign_order,
            status,
        ) in sig_rows
        {
            if status == "PENDING" {
                blocking.insert(doc_id);
            }
            let key = match signed_document_id {
                Some(signed_id) if field_doc_ids.contains(&signed_id) => signed_id,
                _ => doc_id,
            };
            sig_map.entry(key).or_default().push(FieldSignatureStatus {
                requirement_id,
                signer_name,
                signer_user_id,
                sign_order,
                status,
            });
        }
        (blocking, sig_map)
    };

    // Fields marked `requires_signature` block completion even before any requirement
    // has been attached at all — mirrors `process::db::load_state`.
    let step_info_map_for_gating = process::state_machine::build_step_info_map(template);
    let require_sig_doc_ids: Vec<Uuid> = step_info_map_for_gating
        .values()
        .flat_map(|info| &info.fields)
        .filter(|f| f.requires_signature && f.r#type == process::FieldType::File)
        .filter_map(|f| {
            latest_field_states
                .get(&f.field_id)
                .and_then(|fs| fs.document_id)
        })
        .collect();
    if !require_sig_doc_ids.is_empty() {
        // Attached = has its own requirement, or is the signed output of one.
        let unattached: Vec<Uuid> = sqlx::query_scalar(
            "SELECT d.document_id FROM UNNEST($1::uuid[]) AS d(document_id) \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM document_signature_requirements r \
                 WHERE r.document_id = d.document_id \
             ) \
             AND NOT EXISTS ( \
                 SELECT 1 FROM document_signature_requirements r2 \
                 WHERE r2.signed_document_id = d.document_id \
             )",
        )
        .bind(&require_sig_doc_ids)
        .fetch_all(&mut *tx)
        .await
        .internal_err()?;
        blocking_document_ids.extend(unattached);
    }

    Ok((blocking_document_ids, signature_map))
}

/// Which steps need action right now, and from whom. Steps with no fields never surface.
fn compute_next_actions(
    statuses: &HashMap<i32, process::state_machine::StepStatus>,
    step_info_map: &HashMap<i32, process::state_machine::StepInfo>,
) -> Vec<NextActionInfo> {
    let mut next_actions = Vec::new();
    for (&step_id, &status) in statuses {
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
                    let assigned_to = match &info.filled_by {
                        process::StepOwner::User => "user".to_string(),
                        process::StepOwner::Admin => "admin".to_string(),
                        process::StepOwner::Designation { code } => code.clone(),
                        process::StepOwner::WingRepresentative => unreachable!(
                            "WingRepresentative is resolved into a concrete Designation before reaching here"
                        ),
                    };
                    next_actions.push(NextActionInfo {
                        template_step_id: step_id,
                        name: info.name.clone(),
                        assigned_to,
                    });
                }
                _ => {}
            }
        }
    }
    next_actions
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
    let can_view_all = !payload.personal_only && super::can_view_all_processes(&user);
    let actor = build_actor_context(&state.permissions_manager, &user).await;

    let (process_id, template_version_id, ownership_id, template, newly_opened_step_ids) =
        resolve_process_target(
            &mut tx,
            &state,
            &user,
            &payload,
            can_view_all,
            &actor.designation_codes,
        )
        .await?;

    let steps = process::db::load_steps(&mut tx, process_id)
        .await
        .internal_err()?;

    let docs = crate::document::DocumentManager::get_documents_for_process(&mut *tx, process_id)
        .await
        .unwrap_or_default();

    let doc_title_map: HashMap<Uuid, (String, Option<String>)> = docs
        .into_iter()
        .map(|doc| (doc.document_id, (doc.title, doc.content_type)))
        .collect();

    let mut example_doc_ids = Vec::new();
    collect_example_document_ids(&template.fields, &template.substeps, &mut example_doc_ids);
    let example_doc_title_map: HashMap<Uuid, String> = if example_doc_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (Uuid, String)>(
            "SELECT document_id, title FROM documents WHERE document_id = ANY($1) AND category = 'PUBLIC' AND NOT is_deleted",
        )
        .bind(&example_doc_ids)
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    };

    let mut designation_codes = Vec::new();
    collect_designation_codes(&template.substeps, &mut designation_codes);
    let designation_label_map: HashMap<String, String> = if designation_codes.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (String, String)>(
            "SELECT code, label FROM designations WHERE code = ANY($1)",
        )
        .bind(&designation_codes)
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    };

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
            .map(|(t, _)| t.clone());

        field_history_map
            .entry(row.template_field_id)
            .or_default()
            .push(FieldHistoryEntry {
                text_value: row.text_value,
                document_id: row.document_id,
                document_title: title,
                updated_at: row.updated_at,
                actor_email: row.actor_email,
                acted_as_designation: row.acted_as_designation,
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

    // Derived from the review history already fetched above, no second query.
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

    let (blocking_document_ids, signature_map) =
        compute_signature_blocking_state(&mut tx, &template, &latest_field_states).await?;

    let proc_state = process::state_machine::ProcessStateInput {
        steps,
        fields: latest_field_states,
        reviews: reviews_by_step,
        blocking_document_ids,
    };
    let statuses = process::state_machine::compute_statuses(&template, &proc_state);

    let step_info_map = process::state_machine::build_step_info_map(&template);
    let next_actions = compute_next_actions(&statuses, &step_info_map);

    let step_map_ctx = StepMapContext {
        statuses: &statuses,
        field_history_map: &field_history_map,
        latest_field_states: &proc_state.fields,
        doc_title_map: &doc_title_map,
        example_doc_title_map: &example_doc_title_map,
        signature_map: &signature_map,
        designation_label_map: &designation_label_map,
    };
    let mut steps_res = Vec::new();
    if let Some(ref top_steps) = template.substeps {
        for top_step in top_steps {
            steps_res.push(map_step_template(top_step, &step_map_ctx));
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

    let (admin_comments, audit_log) = if can_view_all {
        (
            get_admin_comments(&mut tx, process_id)
                .await
                .unwrap_or_default(),
            process::audit::get_audit_log(&mut tx, process_id)
                .await
                .unwrap_or_default(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    let sla_info = process::db::get_process_info(&mut tx, process_id)
        .await
        .internal_err()?;
    let inward_reg_no = sla_info
        .as_ref()
        .map(|info| info.inward_reg_no.clone())
        .unwrap_or_default();
    let (effective_due_date, is_sla_paused) = match sla_info {
        Some(info) => {
            let holidays = process::db::get_holidays(&mut tx).await.internal_err()?;
            let due = info.sla_due_date.map(|d| {
                process::working_days::add_working_days(d, info.sla_paused_working_days, &holidays)
            });
            (due, info.sla_paused_at.is_some())
        }
        None => (None, false),
    };
    let tat_actual_working_days = process::db::compute_tat_actual(&mut tx, process_id)
        .await
        .internal_err()?;

    tx.commit().await.internal_err()?;
    super::notify_newly_opened_designation_steps(
        &state.permissions_manager,
        &template,
        process_id,
        &newly_opened_step_ids,
    )
    .await;

    Ok(Json(ProcessStateResponse {
        process_id,
        inward_reg_no,
        template_version_id,
        process_name: template.name.clone(),
        ownership_id,
        flat_label,
        is_completed,
        next_actions,
        reviews,
        admin_comments,
        audit_log,
        effective_due_date,
        is_sla_paused,
        tat_actual_working_days,
        steps: steps_res,
    }))
}

#[derive(Deserialize)]
pub struct AuditLogPdfRequest {
    pub process_id: Uuid,
}

fn audit_event_label(event_type: &str) -> String {
    process::audit::AuditEventType::from_db_str(event_type)
        .map(|e| e.label().to_string())
        .unwrap_or_else(|| event_type.to_string())
}

/// POST /api/v1/process/audit-log-pdf
/// Same visibility as `admin_comments`/`audit_log` on `/process/state` — reviewer or
/// view permission, any process (admin oversight, not scoped to a specific flat).
pub async fn audit_log_pdf(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AuditLogPdfRequest>,
) -> Result<axum::response::Response, ApiError> {
    if !super::can_view_all_processes(&user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Not authorized to view the audit trail".to_string()),
        ));
    }

    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    let (info, template) = load_authorized_process(
        &mut conn,
        &user,
        payload.process_id,
        true,
        &std::collections::HashSet::new(),
    )
    .await?;

    let flat_label = if let Some(oid) = info.ownership_id {
        get_flat_info_for_ownership(&mut conn, oid)
            .await
            .internal_err()?
            .map(|f| f.label)
    } else {
        None
    };

    let audit_log = process::audit::get_audit_log(&mut conn, payload.process_id)
        .await
        .internal_err()?;

    let field_history = process::db::get_field_history(&mut conn, payload.process_id)
        .await
        .internal_err()?;

    let mut designation_codes: Vec<String> = audit_log
        .iter()
        .filter_map(|e| e.acted_as_designation.clone())
        .chain(
            field_history
                .iter()
                .filter_map(|h| h.acted_as_designation.clone()),
        )
        .collect();
    designation_codes.sort();
    designation_codes.dedup();
    let designation_label_map: HashMap<String, String> = if designation_codes.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (String, String)>(
            "SELECT code, label FROM designations WHERE code = ANY($1)",
        )
        .bind(&designation_codes)
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    };
    let designation_label = |code: Option<String>| {
        code.and_then(|c| designation_label_map.get(&c).cloned().or(Some(c)))
    };

    let entries: Vec<accounting::pdf::AuditPdfEntry> = audit_log
        .into_iter()
        .map(|e| accounting::pdf::AuditPdfEntry {
            created_at: e.created_at,
            event_label: audit_event_label(&e.event_type),
            actor_email: e.actor_email,
            detail: e.detail,
            acted_as_designation: designation_label(e.acted_as_designation),
        })
        .collect();

    let field_names: HashMap<i32, String> = process::state_machine::build_step_info_map(&template)
        .values()
        .flat_map(|info| info.fields.iter())
        .map(|f| (f.field_id, f.name.clone()))
        .collect();

    let field_changes: Vec<accounting::pdf::FieldChangePdfEntry> = field_history
        .into_iter()
        .map(|h| {
            let value = match h.text_value.as_deref() {
                Some("true") => "Yes".to_string(),
                Some("false") => "No".to_string(),
                Some(other) => other.to_string(),
                None if h.document_id.is_some() => "Document uploaded".to_string(),
                None => "(cleared)".to_string(),
            };
            accounting::pdf::FieldChangePdfEntry {
                created_at: h.updated_at,
                field_name: field_names
                    .get(&h.template_field_id)
                    .cloned()
                    .unwrap_or_else(|| format!("Field {}", h.template_field_id)),
                value,
                actor_email: h.actor_email,
                acted_as_designation: designation_label(h.acted_as_designation),
            }
        })
        .collect();

    let bytes = accounting::pdf::render_process_audit_pdf(
        &template.name,
        flat_label.as_deref(),
        &payload.process_id.to_string(),
        &entries,
        &field_changes,
    );

    Ok(pdf_response(
        &format!("audit-trail-{}.pdf", payload.process_id),
        bytes,
    ))
}
