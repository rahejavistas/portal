//! Read-only process views: the process list and the full state tree for one process.

use super::{load_authorized_process, ApiError, ResultExt};
use crate::extractors::SessionUser;
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
    /// When starting a NEW process (no `process_id`): which `process_templates` row
    /// to start — always starts against whichever version is currently active for it.
    pub template_id: Option<i32>,
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
    pub r#type: String,
    pub filled_by: String,
    pub is_optional: bool,
    /// Template-declared: this document field cannot count as complete until a
    /// reviewer has attached at least one signer AND every attached signer has
    /// signed. See `process::db::load_state`'s matching gating logic.
    pub requires_signature: bool,
    /// Designation codes the "Add Signers" picker should scope to for this field
    /// (empty = no restriction). UI-only, not enforced.
    pub required_signs_from_designations: Vec<String>,
    pub value: Option<FieldValue>,
    pub history: Vec<FieldHistoryEntry>,
    /// Signers a process reviewer has attached to the current document (empty until a
    /// document has been submitted AND a reviewer has attached at least one signer via
    /// the process review screen — signature requirements are never template-declared).
    pub signature_status: Vec<FieldSignatureStatus>,
}

#[derive(Serialize)]
pub struct StepResponse {
    pub template_step_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: process::state_machine::StepStatus,
    pub is_review_required: bool,
    pub fields: Vec<FieldResponse>,
    pub substeps: Vec<StepResponse>,
}

#[derive(Serialize)]
pub struct ProcessStateResponse {
    pub process_id: Uuid,
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
pub struct ProcessListResponse {
    pub processes: Vec<process::db::ProcessListItem>,
    pub flats: Vec<UserFlatResponse>,
}

/// For a resident user, resolve which ownership to use and find-or-create a process instance.
/// `template_id` identifies the process TYPE, not a specific version — the process
/// always starts against whichever version is currently active for it.
async fn resolve_or_init_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    template_id: i32,
    flat_id: Option<i32>,
) -> Result<(Uuid, i32, Option<i32>), ApiError> {
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

    let process_id = process::db::init_process(
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
    Ok((process_id, version_id, selected_ownership))
}

/// Recursively map step templates to the response structure.
fn map_step_template(
    step_tpl: &process::ProcessStepTemplate,
    statuses: &HashMap<i32, process::state_machine::StepStatus>,
    field_history_map: &HashMap<i32, Vec<FieldHistoryEntry>>,
    latest_field_states: &HashMap<i32, process::state_machine::FieldState>,
    doc_title_map: &HashMap<Uuid, String>,
    signature_map: &HashMap<Uuid, Vec<FieldSignatureStatus>>,
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

            let signature_status = latest
                .and_then(|l| l.document_id)
                .and_then(|id| signature_map.get(&id))
                .cloned()
                .unwrap_or_default();

            fields.push(FieldResponse {
                template_field_id: field_id,
                name: f.name.clone(),
                r#type: f.r#type.clone(),
                filled_by: f.filled_by.clone(),
                is_optional: f.is_optional,
                requires_signature: f.requires_signature,
                required_signs_from_designations: f.required_signs_from_designations.clone(),
                value,
                history,
                signature_status,
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
                signature_map,
            ));
        }
    }

    StepResponse {
        template_step_id: step_id,
        name: step_tpl.name.clone(),
        description: step_tpl.description.clone(),
        status,
        is_review_required: step_tpl.is_review_required,
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
        if let Ok(template) =
            process::template_store::load_template_version(&mut conn, item.template_version_id)
                .await
        {
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

    // --- Resolve process_id, template_version_id, ownership_id ---
    let (process_id, template_version_id, ownership_id) = match payload.process_id {
        Some(pid) => {
            // A user currently or previously attached as a signer on some document in
            // this exact process gets read-only access to just this one process, even
            // without process.reviewer/process.view — deliberately narrower than
            // `can_view_all` (doesn't affect admin-comment visibility or anything else
            // gated on that flag below).
            let is_authorized = can_view_all
                || state
                    .document_manager
                    .signer_has_requirement_for_process(user.user_id, pid)
                    .await
                    .internal_err()?;
            let (info, _) = load_authorized_process(&mut tx, &user, pid, is_authorized).await?;
            (pid, info.template_version_id, info.ownership_id)
        }
        None => {
            let target_template_id = payload.template_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json("Either process_id or template_id must be provided".to_string()),
                )
            })?;
            resolve_or_init_process(&mut tx, &user, target_template_id, payload.flat_id).await?
        }
    };

    // --- Load template and workflow ---
    let template = process::template_store::load_template_version(&mut tx, template_version_id)
        .await
        .map_err(|e| {
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

    let field_doc_ids: Vec<Uuid> = latest_field_states
        .values()
        .filter_map(|f| f.document_id)
        .collect();
    let (blocking_document_ids, signature_map): (
        std::collections::HashSet<Uuid>,
        HashMap<Uuid, Vec<FieldSignatureStatus>>,
    ) = if field_doc_ids.is_empty() {
        (std::collections::HashSet::new(), HashMap::new())
    } else {
        // Requirement rows are permanently keyed to the ORIGINAL (pre-signing)
        // document_id — once every signer has signed, the field gets repointed at
        // `signed_document_id` instead (see `attach_replacement_and_recompute_gating`),
        // so a lookup by the field's CURRENT document alone would find nothing for an
        // already-fully-signed field. Match on either id, and key the map by whichever
        // one is actually the field's current document, so `signature_map.get(current_doc)`
        // still resolves after the repoint.
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
    let mut blocking_document_ids = blocking_document_ids;

    // Fields marked `requires_signature` block completion even before any requirement
    // has been attached at all — mirrors `process::db::load_state`.
    let step_info_map_for_gating = process::state_machine::build_step_info_map(&template);
    let require_sig_doc_ids: Vec<Uuid> = step_info_map_for_gating
        .values()
        .flat_map(|info| &info.fields)
        .filter(|f| f.requires_signature && f.r#type == "document")
        .filter_map(|f| {
            latest_field_states
                .get(&f.field_id)
                .and_then(|fs| fs.document_id)
        })
        .collect();
    if !require_sig_doc_ids.is_empty() {
        // A document with no requirement of its own is "unattached" UNLESS it's
        // itself the signed output of a completed requirement on some other (now
        // historical) document — see the matching comment in process::db::load_state.
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

    let proc_state = process::state_machine::ProcessStateInput {
        steps,
        fields: latest_field_states,
        reviews: reviews_by_step,
        blocking_document_ids,
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
                &signature_map,
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
        template_version_id,
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
