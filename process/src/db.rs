use chrono::Datelike;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::state_machine::{
    self, FieldState, ProcessStateInput, ReviewState, StepStateDb, StepStatus,
};

// ==========================================
// Top-level process operations
// ==========================================

/// Next human-facing inward register number, formatted "INW-{year}-{seq:06}".
/// The sequence is never reset per year — a year rollover just means the next
/// number carries a new year prefix while the underlying counter keeps climbing.
pub async fn next_inward_reg_no(conn: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT nextval('inward_reg_no_seq')")
        .fetch_one(conn)
        .await?;
    let year = chrono::Utc::now().year();
    Ok(format!("INW-{}-{:06}", year, n))
}

/// Create a new process and all its step rows.
pub async fn init_process(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
    template_version_id: i32,
    template: &crate::ProcessTemplate,
    ownership_id: Option<i32>,
) -> Result<InitProcessOutcome, sqlx::Error> {
    let inward_reg_no = next_inward_reg_no(&mut *conn).await?;
    let process_id: Uuid = sqlx::query_scalar(
        "INSERT INTO processes (inward_reg_no, user_id, template_version_id, ownership_id) \
         VALUES ($1, $2, $3, $4) RETURNING process_id",
    )
    .bind(inward_reg_no)
    .bind(user_id)
    .bind(template_version_id)
    .bind(ownership_id)
    .fetch_one(&mut *conn)
    .await?;

    // Insert root step
    let root_sid: Uuid = sqlx::query_scalar(
        "INSERT INTO process_steps (process_id, template_step_id, parent_step_instance_id, is_completed) \
         VALUES ($1, $2, NULL, FALSE) RETURNING step_instance_id",
    )
    .bind(process_id)
    .bind(template.step_id)
    .fetch_one(&mut *conn)
    .await?;

    // Insert top-level steps and their substeps
    if let Some(ref top_steps) = template.substeps {
        for top_step in top_steps {
            let top_sid: Uuid = sqlx::query_scalar(
                "INSERT INTO process_steps (process_id, template_step_id, parent_step_instance_id, is_completed) \
                 VALUES ($1, $2, $3, FALSE) RETURNING step_instance_id",
            )
            .bind(process_id)
            .bind(top_step.step_id)
            .bind(root_sid)
            .fetch_one(&mut *conn)
            .await?;

            if let Some(ref substeps) = top_step.substeps {
                for substep in substeps {
                    sqlx::query(
                        "INSERT INTO process_steps (process_id, template_step_id, parent_step_instance_id, is_completed) \
                         VALUES ($1, $2, $3, FALSE)",
                    )
                    .bind(process_id)
                    .bind(substep.step_id)
                    .bind(top_sid)
                    .execute(&mut *conn)
                    .await?;
                }
            }
        }
    }

    // Set started_at for initially-open steps
    sync_start_times(&mut *conn, process_id, template).await?;

    // Nothing existed before creation, so "newly opened" is just whatever's Open now.
    let initial_statuses = state_machine::compute_statuses(template, &ProcessStateInput::default());
    let newly_opened_step_ids: Vec<i32> = initial_statuses
        .into_iter()
        .filter(|(_, status)| *status == StepStatus::Open)
        .map(|(step_id, _)| step_id)
        .collect();

    Ok(InitProcessOutcome {
        process_id,
        newly_opened_step_ids,
    })
}

/// Outcome of a successful `init_process` call.
pub struct InitProcessOutcome {
    pub process_id: Uuid,
    pub newly_opened_step_ids: Vec<i32>,
}

/// Outcome of a successful `save_step` call.
pub struct SaveStepOutcome {
    pub status: StepStatus,
    pub newly_opened_step_ids: Vec<i32>,
}

/// Save fields for a step. Validates, persists fields, and updates completion.
/// Returns the new step status.
pub async fn save_step(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    step_id: i32,
    fields: &[state_machine::SaveFieldInput],
    actor: &state_machine::ActorContext,
    actor_user_id: i32,
) -> Result<SaveStepOutcome, String> {
    let state = load_state(conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;
    let before_statuses = state_machine::compute_statuses(template, &state);
    let result = state_machine::validate_save(template, &state, step_id, fields, actor)?;

    persist_fields(
        &mut *conn,
        process_id,
        step_id,
        &result.fields_to_save,
        Some(actor_user_id),
        result.acted_as_designation.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    // Fresh load, not result.steps_to_update — a newly-introduced requires_signature
    // document can't be seen as blocking until it's actually persisted.
    let mut post_save_state = load_state(&mut *conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;
    let steps_to_update =
        state_machine::compute_completion_updates(template, &post_save_state, step_id);
    let status = steps_to_update
        .iter()
        .find(|(sid, _)| *sid == step_id)
        .map(|(_, completed)| {
            if *completed {
                if state_machine::build_step_info_map(template)
                    .get(&step_id)
                    .map(|i| i.requires_review(template.step_id))
                    .unwrap_or(false)
                {
                    StepStatus::PendingReview
                } else {
                    StepStatus::Completed
                }
            } else {
                StepStatus::Open
            }
        })
        .unwrap_or(result.new_status);

    persist_step_completions(&mut *conn, process_id, &steps_to_update)
        .await
        .map_err(|e| e.to_string())?;

    // Patch in-memory instead of reloading — persist_step_completions above just wrote
    // exactly these values, so post_save_state.steps is otherwise still fresh.
    for &(sid, completed) in &steps_to_update {
        if let Some(s) = post_save_state.steps.get_mut(&sid) {
            s.is_completed = completed;
        }
    }
    persist_start_times_and_turn(&mut *conn, process_id, template, &post_save_state)
        .await
        .map_err(|e| e.to_string())?;

    if matches!(status, StepStatus::Completed | StepStatus::PendingReview) {
        crate::audit::insert_audit_event(
            &mut *conn,
            process_id,
            Some(step_id),
            crate::audit::AuditEventType::StepCompleted,
            None,
            Some(actor_user_id),
            result.acted_as_designation.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    // Resume on an actual completion of this step (whether that hands the process back
    // to the admin for re-review, or — if this was the last outstanding step — closes
    // the process directly): a partial/incomplete save must never resume the clock.
    if matches!(status, StepStatus::Completed | StepStatus::PendingReview) {
        resume_sla_clock_if_paused(&mut *conn, process_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    let post_save_turn = state_machine::determine_turn(template, &post_save_state);
    maybe_start_sla_clock(&mut *conn, process_id, template, post_save_turn)
        .await
        .map_err(|e| e.to_string())?;

    let after_statuses = state_machine::compute_statuses(template, &post_save_state);
    let newly_opened_step_ids =
        state_machine::newly_opened_steps(&before_statuses, &after_statuses);

    Ok(SaveStepOutcome {
        status,
        newly_opened_step_ids,
    })
}

/// Parameters for recording an admin review.
/// Bundled into a struct to keep `review_step` within clippy argument limits.
pub struct ReviewStepParams<'a> {
    pub step_id: i32,
    pub reviewer_user_id: Option<i32>,
    pub is_approved: bool,
    pub notes: Option<&'a str>,
    /// Steps the admin flagged for resident attention. Informational only.
    pub flagged_step_ids: &'a [i32],
    /// The designation that authorized this review, if `step_id` is `Designation`-owned.
    pub acted_as_designation: Option<&'a str>,
}

/// Outcome of a successful `review_step` call.
pub struct ReviewStepOutcome {
    /// Whether the step is now completed (`process_steps.is_completed`).
    pub is_completed: bool,
    pub newly_opened_step_ids: Vec<i32>,
}

/// Record an admin review (approve/reject).
pub async fn review_step(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    mut params: ReviewStepParams<'_>,
    actor: &state_machine::ActorContext,
) -> Result<ReviewStepOutcome, String> {
    let state = load_state(conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;
    let before_statuses = state_machine::compute_statuses(template, &state);
    let review_result = state_machine::compute_review_updates(
        template,
        &state,
        params.step_id,
        params.is_approved,
        actor,
    )?;
    let steps_to_update = review_result.steps_to_update;
    let is_completed = review_result.is_completed;
    params.acted_as_designation = review_result.acted_as_designation.as_deref();

    persist_step_completions(&mut *conn, process_id, &steps_to_update)
        .await
        .map_err(|e| e.to_string())?;

    insert_review(&mut *conn, process_id, &params)
        .await
        .map_err(|e| e.to_string())?;

    if params.is_approved {
        resume_sla_clock_if_paused(&mut *conn, process_id)
            .await
            .map_err(|e| e.to_string())?;
    }

    sync_start_times(&mut *conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;

    // Fresh load: insert_review above just added a row compute_statuses depends on.
    let state = load_state(&mut *conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;
    let after_statuses = state_machine::compute_statuses(template, &state);
    let newly_opened_step_ids =
        state_machine::newly_opened_steps(&before_statuses, &after_statuses);

    Ok(ReviewStepOutcome {
        is_completed,
        newly_opened_step_ids,
    })
}

/// Reopen a completed/pending-review step by inserting rejection reviews and resetting completion.
/// Returns any step_ids that newly became Open as a result (e.g. the reopened step itself).
pub async fn reopen_step(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    step_id: i32,          // template_step_id of the step to reopen
    reviewer_user_id: i32, // users.user_id of the reviewer performing the reopen
    custom_note: Option<&str>,
    actor: &state_machine::ActorContext,
) -> Result<Vec<i32>, String> {
    let mut state = load_state(conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;
    let before_statuses = state_machine::compute_statuses(template, &state);
    let reopen_result = state_machine::compute_reopen_updates(template, &state, step_id, actor)?;
    let steps_to_reset = reopen_result.steps_to_reset;
    let statuses = state_machine::compute_statuses(template, &state);

    // Insert rejection reviews for steps that are currently closed. Every row records the
    // designation that authorized this reopen action, not each cascaded step's own owner.
    for &sid in &steps_to_reset {
        let is_closed = matches!(
            statuses.get(&sid),
            Some(StepStatus::Completed) | Some(StepStatus::PendingReview)
        );
        if is_closed {
            insert_review(
                &mut *conn,
                process_id,
                &ReviewStepParams {
                    step_id: sid,
                    reviewer_user_id: Some(reviewer_user_id),
                    is_approved: false,
                    notes: custom_note,
                    flagged_step_ids: &[],
                    acted_as_designation: reopen_result.acted_as_designation.as_deref(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    // Reset completion
    let updates: Vec<(i32, bool)> = steps_to_reset.iter().map(|&id| (id, false)).collect();
    persist_step_completions(&mut *conn, process_id, &updates)
        .await
        .map_err(|e| e.to_string())?;

    // Patch the already-loaded state with the resets above instead of reloading it,
    // to derive the new current_turn.
    for &sid in &steps_to_reset {
        if let Some(s) = state.steps.get_mut(&sid) {
            s.is_completed = false;
        }
    }
    persist_current_turn(
        &mut *conn,
        process_id,
        state_machine::determine_turn(template, &state),
    )
    .await
    .map_err(|e| e.to_string())?;

    let after_statuses = state_machine::compute_statuses(template, &state);
    Ok(state_machine::newly_opened_steps(
        &before_statuses,
        &after_statuses,
    ))
}

/// Compute statuses for all steps in a process.
pub async fn get_statuses(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<HashMap<i32, StepStatus>, String> {
    let state = load_state(conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state_machine::compute_statuses(template, &state))
}

/// Compute whose turn it is to act on a process.
pub async fn get_process_turn(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<state_machine::ProcessTurn, sqlx::Error> {
    let state = load_state(conn, process_id, template).await?;
    Ok(state_machine::determine_turn(template, &state))
}

// ==========================================
// Query helpers (public, read-only)
// ==========================================

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProcessInfo {
    pub user_id: i32,
    pub template_version_id: i32,
    pub ownership_id: Option<i32>,
    pub inward_reg_no: String,
    pub sla_due_date: Option<chrono::NaiveDate>,
    pub sla_paused_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sla_paused_working_days: i32,
}

pub async fn get_process_info(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Option<ProcessInfo>, sqlx::Error> {
    sqlx::query_as(
        "SELECT user_id, template_version_id, ownership_id, inward_reg_no, \
                sla_due_date, sla_paused_at, sla_paused_working_days \
         FROM processes WHERE process_id = $1",
    )
    .bind(process_id)
    .fetch_optional(conn)
    .await
}

/// `(process_id, template_version_id, ownership_id)` for every process currently
/// waiting on a non-resident action — the only ones that could have a designation-owned
/// step open right now.
pub async fn processes_with_admin_turn(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<(Uuid, i32, Option<i32>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT process_id, template_version_id, ownership_id FROM processes \
         WHERE current_turn = 'admin'",
    )
    .fetch_all(conn)
    .await
}

/// Look up a user's email address by their user_id.
/// Used by notification helpers to build email payloads.
pub async fn get_user_email(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT email FROM users WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(conn)
        .await
}

#[derive(Clone, serde::Serialize, sqlx::FromRow)]
pub struct ProcessReviewHistory {
    pub review_id: Uuid,
    pub template_step_id: i32,
    pub admin_email: Option<String>,
    pub is_approved: bool,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Template step IDs the admin flagged during this review as needing
    /// resident attention. Purely informational — does not affect step state.
    pub flagged_step_ids: Vec<i32>,
    pub acted_as_designation: Option<String>,
}

pub async fn get_process_reviews(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<ProcessReviewHistory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT pr.review_id, ps.template_step_id, u.email as admin_email, \
                pr.is_approved, pr.notes, pr.created_at, pr.flagged_step_ids, pr.acted_as_designation \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         LEFT JOIN users u ON pr.reviewer_user_id = u.user_id \
         WHERE ps.process_id = $1 \
         ORDER BY pr.created_at ASC",
    )
    .bind(process_id)
    .fetch_all(conn)
    .await
}

#[derive(sqlx::FromRow)]
pub struct ProcessFieldHistoryRow {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub actor_email: Option<String>,
    pub acted_as_designation: Option<String>,
}

pub async fn get_field_history(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<ProcessFieldHistoryRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT pf.template_field_id, pf.text_value, pf.document_id, pf.updated_at, u.email AS actor_email, \
                pf.acted_as_designation \
         FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         LEFT JOIN users u ON u.user_id = pf.updated_by \
         WHERE ps.process_id = $1 \
         ORDER BY pf.template_field_id, pf.updated_at DESC, pf.field_instance_id DESC",
    )
    .bind(process_id)
    .fetch_all(conn)
    .await
}

// ==========================================
// Process listing
// ==========================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct ProcessListItem {
    pub process_id: Uuid,
    pub inward_reg_no: String,
    pub template_version_id: i32,
    pub template_name: String,
    pub ownership_id: Option<i32>,
    pub flat_label: Option<String>,
    pub user_email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub steps_total: i64,
    pub steps_completed: i64,
    /// NULL for a process that predates this column.
    pub current_turn: Option<String>,
    #[sqlx(default)]
    pub status: Option<String>,
    /// Set only for the reviewer-facing list (`can_view_all`) — whether the caller
    /// personally can act on this process right now, not just "is it admin's turn".
    #[sqlx(default)]
    pub pending_on_caller: Option<bool>,
    pub tat_working_days: Option<i32>,
    pub sla_started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sla_due_date: Option<chrono::NaiveDate>,
    pub sla_paused_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sla_paused_working_days: i32,
}

const PROCESS_LIST_QUERY_BASE: &str =
    "SELECT p.process_id, p.inward_reg_no, p.template_version_id, pt.name AS template_name, p.ownership_id, \
            CASE WHEN fo.flat_id IS NOT NULL \
                 THEN f.wing || '-' || f.flat_number \
                 ELSE NULL END AS flat_label, \
            u.email AS user_email, \
            p.created_at, \
            p.updated_at, \
            p.current_turn, \
            p.tat_working_days, p.sla_started_at, p.sla_due_date, p.sla_paused_at, p.sla_paused_working_days, \
            COUNT(ps.step_instance_id) FILTER (WHERE ps.parent_step_instance_id IS NOT NULL) AS steps_total, \
            COUNT(ps.step_instance_id) FILTER (WHERE ps.parent_step_instance_id IS NOT NULL AND ps.is_completed = TRUE) AS steps_completed \
     FROM processes p \
     JOIN users u ON p.user_id = u.user_id \
     JOIN process_template_versions ptv ON ptv.version_id = p.template_version_id \
     JOIN process_templates pt ON pt.template_id = ptv.template_id \
     LEFT JOIN flat_ownerships fo ON p.ownership_id = fo.ownership_id \
     LEFT JOIN flats f ON fo.flat_id = f.flat_id \
     LEFT JOIN process_steps ps ON p.process_id = ps.process_id";

const PROCESS_LIST_GROUP_BY: &str = "GROUP BY p.process_id, p.inward_reg_no, p.template_version_id, pt.name, p.ownership_id, fo.flat_id, f.wing, f.flat_number, u.email, p.created_at, p.updated_at, p.current_turn, p.tat_working_days, p.sla_started_at, p.sla_due_date, p.sla_paused_at, p.sla_paused_working_days";

pub async fn list_processes_for_user(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
) -> Result<Vec<ProcessListItem>, sqlx::Error> {
    let query = format!(
        "{} WHERE p.user_id = $1 AND (fo.ownership_id IS NULL OR fo.is_active = TRUE) \
         {} \
         ORDER BY p.created_at DESC",
        PROCESS_LIST_QUERY_BASE, PROCESS_LIST_GROUP_BY
    );
    sqlx::query_as(&query).bind(user_id).fetch_all(conn).await
}

/// Processes for flats where `tenant_user_id` is the active tenant; caller filters
/// by Tenant-step.
pub async fn list_processes_for_tenant(
    conn: &mut sqlx::PgConnection,
    tenant_user_id: i32,
) -> Result<Vec<ProcessListItem>, sqlx::Error> {
    let query = format!(
        "{} WHERE p.ownership_id IN ( \
             SELECT t.ownership_id FROM tenant_ownerships t \
             JOIN flat_ownerships fo ON fo.ownership_id = t.ownership_id \
             WHERE t.tenant_user_id = $1 AND t.is_active = TRUE \
               AND t.expires_at >= CURRENT_DATE AND fo.is_active = TRUE \
         ) \
         {} \
         ORDER BY p.created_at DESC",
        PROCESS_LIST_QUERY_BASE, PROCESS_LIST_GROUP_BY
    );
    sqlx::query_as(&query)
        .bind(tenant_user_id)
        .fetch_all(conn)
        .await
}

/// Ordered by `inward_reg_no` ascending — the FIFO queue a compliance manager
/// works through, not creation time.
pub async fn list_all_processes(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<ProcessListItem>, sqlx::Error> {
    let query = format!(
        "{} \
         {} \
         ORDER BY p.inward_reg_no ASC",
        PROCESS_LIST_QUERY_BASE, PROCESS_LIST_GROUP_BY
    );
    sqlx::query_as(&query).fetch_all(conn).await
}

// ==========================================
// Flat/ownership helpers
// ==========================================

/// A flat's ID and its display label (e.g. "A-101"), looked up by ownership.
pub struct OwnershipFlatInfo {
    pub flat_id: i32,
    pub label: String,
}

pub async fn get_flat_info_for_ownership(
    conn: &mut sqlx::PgConnection,
    ownership_id: i32,
) -> Result<Option<OwnershipFlatInfo>, sqlx::Error> {
    let row: Option<(i32, String)> = sqlx::query_as(
        "SELECT f.flat_id, f.wing || '-' || f.flat_number \
         FROM flat_ownerships fo \
         JOIN flats f ON fo.flat_id = f.flat_id \
         WHERE fo.ownership_id = $1",
    )
    .bind(ownership_id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|(flat_id, label)| OwnershipFlatInfo { flat_id, label }))
}

/// Every wing's representative designation code, system-wide.
pub async fn get_all_wing_representative_codes(
    conn: &mut sqlx::PgConnection,
) -> Result<HashSet<String>, sqlx::Error> {
    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT d.code FROM wings w \
         JOIN designations d ON d.designation_id = w.representative_designation_id",
    )
    .fetch_all(conn)
    .await?;
    Ok(codes.into_iter().collect())
}

/// Whether `user_id` is the current active tenant of the flat behind `ownership_id`.
pub async fn is_active_tenant_of_ownership(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
    ownership_id: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS ( \
            SELECT 1 FROM tenant_ownerships t \
            JOIN flat_ownerships fo ON fo.ownership_id = t.ownership_id \
            WHERE t.tenant_user_id = $1 AND t.ownership_id = $2 AND t.is_active = TRUE \
              AND t.expires_at >= CURRENT_DATE AND fo.is_active = TRUE \
         )",
    )
    .bind(user_id)
    .bind(ownership_id)
    .fetch_one(conn)
    .await
}

/// Email of `ownership_id`'s flat's current active tenant, if any.
pub async fn get_active_tenant_email(
    conn: &mut sqlx::PgConnection,
    ownership_id: i32,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT u.email FROM tenant_ownerships t \
         JOIN users u ON u.user_id = t.tenant_user_id \
         JOIN flat_ownerships fo ON fo.ownership_id = t.ownership_id \
         WHERE t.ownership_id = $1 AND t.is_active = TRUE \
           AND t.expires_at >= CURRENT_DATE AND fo.is_active = TRUE",
    )
    .bind(ownership_id)
    .fetch_optional(conn)
    .await
}

/// Whether `ownership_id`'s flat has any active, unexpired tenant at all, regardless
/// of who, with the underlying flat ownership still active.
pub async fn has_active_tenant(
    conn: &mut sqlx::PgConnection,
    ownership_id: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS ( \
            SELECT 1 FROM tenant_ownerships t \
            JOIN flat_ownerships fo ON fo.ownership_id = t.ownership_id \
            WHERE t.ownership_id = $1 AND t.is_active = TRUE \
              AND t.expires_at >= CURRENT_DATE AND fo.is_active = TRUE \
         )",
    )
    .bind(ownership_id)
    .fetch_one(conn)
    .await
}

/// Batched form of `load_steps`, keyed by `process_id`.
pub async fn load_steps_for_processes(
    conn: &mut sqlx::PgConnection,
    process_ids: &[Uuid],
) -> Result<HashMap<Uuid, HashMap<i32, StepStateDb>>, sqlx::Error> {
    let rows: Vec<(Uuid, i32, bool, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT process_id, template_step_id, is_completed, started_at \
         FROM process_steps WHERE process_id = ANY($1)",
    )
    .bind(process_ids)
    .fetch_all(conn)
    .await?;

    let mut by_process: HashMap<Uuid, HashMap<i32, StepStateDb>> = HashMap::new();
    for (pid, tid, completed, started) in rows {
        by_process.entry(pid).or_default().insert(
            tid,
            StepStateDb {
                is_completed: completed,
                started_at: started,
            },
        );
    }
    Ok(by_process)
}

/// Batched form of `get_process_reviews`, keyed by `process_id`.
pub async fn get_process_reviews_for_processes(
    conn: &mut sqlx::PgConnection,
    process_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<ProcessReviewHistory>>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        Uuid,
        i32,
        Option<String>,
        bool,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Vec<i32>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT ps.process_id, pr.review_id, ps.template_step_id, u.email as admin_email, \
                pr.is_approved, pr.notes, pr.created_at, pr.flagged_step_ids, pr.acted_as_designation \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         LEFT JOIN users u ON pr.reviewer_user_id = u.user_id \
         WHERE ps.process_id = ANY($1) \
         ORDER BY pr.created_at ASC",
    )
    .bind(process_ids)
    .fetch_all(conn)
    .await?;

    let mut by_process: HashMap<Uuid, Vec<ProcessReviewHistory>> = HashMap::new();
    for (
        process_id,
        review_id,
        template_step_id,
        admin_email,
        is_approved,
        notes,
        created_at,
        flagged_step_ids,
        acted_as_designation,
    ) in rows
    {
        by_process
            .entry(process_id)
            .or_default()
            .push(ProcessReviewHistory {
                review_id,
                template_step_id,
                admin_email,
                is_approved,
                notes,
                created_at,
                flagged_step_ids,
                acted_as_designation,
            });
    }
    Ok(by_process)
}

pub async fn get_active_ownerships_for_user(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
) -> Result<Vec<(i32, i32)>, sqlx::Error> {
    // (ownership_id, flat_id)
    sqlx::query_as(
        "SELECT ownership_id, flat_id FROM flat_ownerships WHERE user_id = $1 AND is_active = TRUE",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UserFlatInfo {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
}

pub async fn get_user_active_flats(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
) -> Result<Vec<UserFlatInfo>, sqlx::Error> {
    sqlx::query_as(
        "SELECT f.flat_id, f.wing, f.flat_number \
         FROM flat_ownerships fo \
         JOIN flats f ON fo.flat_id = f.flat_id \
         WHERE fo.user_id = $1 AND fo.is_active = TRUE",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await
}

// ==========================================
// Private DB helpers
// ==========================================

/// Per-step completion state (is_completed, started_at) for a process.
pub async fn load_steps(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<HashMap<i32, StepStateDb>, sqlx::Error> {
    // (template_step_id, is_completed, started_at)
    let steps_rows: Vec<(i32, bool, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT template_step_id, is_completed, started_at FROM process_steps WHERE process_id = $1",
    )
    .bind(process_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut steps = HashMap::new();
    for (tid, completed, started) in steps_rows {
        steps.insert(
            tid,
            StepStateDb {
                is_completed: completed,
                started_at: started,
            },
        );
    }
    Ok(steps)
}

/// Load the full process state from DB (steps, fields, reviews).
pub async fn load_state(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<ProcessStateInput, sqlx::Error> {
    let steps = load_steps(&mut *conn, process_id).await?;

    // (template_field_id, text_value, document_id) — latest per field via DISTINCT ON
    let fields_rows: Vec<(i32, Option<String>, Option<Uuid>)> = sqlx::query_as(
        "SELECT DISTINCT ON (pf.template_field_id) pf.template_field_id, pf.text_value, pf.document_id \
         FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 \
         ORDER BY pf.template_field_id, pf.updated_at DESC, pf.field_instance_id DESC",
    )
    .bind(process_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut fields = HashMap::new();
    for (tid, text, doc) in fields_rows {
        fields.insert(
            tid,
            FieldState {
                text_value: text,
                document_id: doc,
            },
        );
    }

    // (template_step_id, is_approved, created_at)
    let reviews_rows: Vec<(i32, bool, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT ps.template_step_id, pr.is_approved, pr.created_at \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1",
    )
    .bind(process_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut reviews: HashMap<i32, Vec<ReviewState>> = HashMap::new();
    for (tid, approved, created) in reviews_rows {
        reviews.entry(tid).or_default().push(ReviewState {
            is_approved: approved,
            created_at: created,
        });
    }

    let field_doc_ids: Vec<Uuid> = fields.values().filter_map(|f| f.document_id).collect();
    let mut blocking_document_ids: std::collections::HashSet<Uuid> = if field_doc_ids.is_empty() {
        std::collections::HashSet::new()
    } else {
        sqlx::query_scalar(
            "SELECT document_id FROM document_signature_requirements \
             WHERE status = 'PENDING' AND document_id = ANY($1)",
        )
        .bind(&field_doc_ids)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect()
    };

    // requires_signature fields block completion until a reviewer attaches a signer.
    let step_info_map = state_machine::build_step_info_map(template);
    let require_sig_doc_ids: Vec<Uuid> = step_info_map
        .values()
        .flat_map(|info| &info.fields)
        .filter(|f| f.requires_signature && f.r#type == crate::FieldType::File)
        .filter_map(|f| fields.get(&f.field_id).and_then(|fs| fs.document_id))
        .collect();
    if !require_sig_doc_ids.is_empty() {
        // A document counts as attached if it either has its own requirement, or is
        // itself the signed output of one (a field repointed at a signed copy).
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
        .fetch_all(&mut *conn)
        .await?;
        blocking_document_ids.extend(unattached);
    }

    inject_no_dues_amounts(&mut *conn, process_id, &step_info_map, &mut fields).await?;

    Ok(ProcessStateInput {
        steps,
        fields,
        reviews,
        blocking_document_ids,
    })
}

/// `NoDues` is resolved live from the dues snapshot every call, never stored in `process_fields`.
pub async fn inject_no_dues_amounts(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    step_info_map: &HashMap<i32, state_machine::StepInfo>,
    fields: &mut HashMap<i32, FieldState>,
) -> Result<(), sqlx::Error> {
    let no_dues_field_ids: Vec<i32> = step_info_map
        .values()
        .flat_map(|info| &info.fields)
        .filter(|f| f.r#type == crate::FieldType::NoDues)
        .map(|f| f.field_id)
        .collect();
    if no_dues_field_ids.is_empty() {
        return Ok(());
    }
    let outstanding = get_outstanding_dues_for_process(conn, process_id).await?;
    for field_id in no_dues_field_ids {
        fields.insert(
            field_id,
            FieldState {
                text_value: Some(outstanding.to_string()),
                document_id: None,
            },
        );
    }
    Ok(())
}

/// Syncs completion for every step gated by a `NoDues` field — nothing else ever saves one.
pub async fn sync_no_dues_step_completions(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<(), sqlx::Error> {
    let step_info_map = state_machine::build_step_info_map(template);
    let no_dues_step_ids: Vec<i32> = step_info_map
        .values()
        .filter(|info| {
            info.fields
                .iter()
                .any(|f| f.r#type == crate::FieldType::NoDues)
        })
        .map(|info| info.step_id)
        .collect();
    if no_dues_step_ids.is_empty() {
        return Ok(());
    }
    for step_id in no_dues_step_ids {
        let state = load_state(&mut *conn, process_id, template).await?;
        let Some(info) = step_info_map.get(&step_id) else {
            continue;
        };
        // A formally admin-reviewed stage must never be silently reopened by this sync.
        let already_completed = state.steps.get(&step_id).map(|s| s.is_completed) == Some(true);
        if info.requires_review(template.step_id) && already_completed {
            continue;
        }
        let updates = state_machine::compute_completion_updates(template, &state, step_id);
        // Only persist steps whose flag actually flips, or completed_at drifts on every read.
        let changed: Vec<(i32, bool)> = updates
            .into_iter()
            .filter(|&(id, completed)| {
                state.steps.get(&id).map(|s| s.is_completed) != Some(completed)
            })
            .collect();
        persist_step_completions(&mut *conn, process_id, &changed).await?;
    }
    Ok(())
}

/// Reads the uploaded dues snapshot, not accounting — no row yet means zero due.
pub async fn get_outstanding_dues_for_process(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<rust_decimal::Decimal, sqlx::Error> {
    let amount: Option<rust_decimal::Decimal> = sqlx::query_scalar(
        "SELECT d.total_due \
         FROM processes p \
         JOIN flat_ownerships fo ON fo.ownership_id = p.ownership_id \
         JOIN flat_dues_from_file d ON d.flat_id = fo.flat_id \
         WHERE p.process_id = $1",
    )
    .bind(process_id)
    .fetch_optional(conn)
    .await?;
    Ok(amount.unwrap_or_default())
}

/// Batched, ownership_id-keyed form of `get_outstanding_dues_for_process` for lists.
pub async fn get_outstanding_dues_for_ownerships(
    conn: &mut sqlx::PgConnection,
    ownership_ids: &[i32],
) -> Result<HashMap<i32, rust_decimal::Decimal>, sqlx::Error> {
    let rows: Vec<(i32, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT fo.ownership_id, d.total_due \
         FROM flat_ownerships fo \
         JOIN flat_dues_from_file d ON d.flat_id = fo.flat_id \
         WHERE fo.ownership_id = ANY($1)",
    )
    .bind(ownership_ids)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Sets started_at for newly-unlocked steps and refreshes processes.current_turn.
async fn sync_start_times(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<(), sqlx::Error> {
    let state = load_state(&mut *conn, process_id, template).await?;
    persist_start_times_and_turn(conn, process_id, template, &state).await
}

/// Same as `sync_start_times` but skips the reload for callers that already have state.
async fn persist_start_times_and_turn(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
) -> Result<(), sqlx::Error> {
    let statuses = state_machine::compute_statuses(template, state);
    let now = chrono::Utc::now();

    for (&step_id, &status) in &statuses {
        if status != StepStatus::Locked {
            let needs_start = state
                .steps
                .get(&step_id)
                .map(|s| s.started_at.is_none())
                .unwrap_or(true);
            if needs_start {
                sqlx::query(
                    "UPDATE process_steps SET started_at = $1 \
                     WHERE process_id = $2 AND template_step_id = $3 AND started_at IS NULL",
                )
                .bind(now)
                .bind(process_id)
                .bind(step_id)
                .execute(&mut *conn)
                .await?;
            }
        }
    }

    let just_closed = persist_current_turn(
        &mut *conn,
        process_id,
        state_machine::determine_turn(template, state),
    )
    .await?;

    if just_closed {
        crate::audit::insert_audit_event(
            &mut *conn,
            process_id,
            None,
            crate::audit::AuditEventType::ProcessClosed,
            None,
            None,
            None,
        )
        .await?;
    }

    Ok(())
}

/// Updates `current_turn`, skipping the write if it's unchanged. Returns whether this
/// call is the transition into `None` — i.e. the process just reached its terminal
/// state for the first time, not a later no-op call while it's already closed.
async fn persist_current_turn(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    turn: state_machine::ProcessTurn,
) -> Result<bool, sqlx::Error> {
    let changed: Option<(String,)> = sqlx::query_as(
        "UPDATE processes SET current_turn = $1 \
         WHERE process_id = $2 AND current_turn IS DISTINCT FROM $1 \
         RETURNING current_turn",
    )
    .bind(turn.as_db_str())
    .bind(process_id)
    .fetch_optional(conn)
    .await?;
    Ok(changed.is_some() && turn == state_machine::ProcessTurn::None)
}

/// Whether `document_id` is still the field's latest value (it may have been
/// superseded by a later save before its signature completed).
pub async fn is_current_field_value(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_field_id: i32,
    document_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM ( \
                 SELECT DISTINCT ON (pf.template_field_id) pf.document_id \
                 FROM process_fields pf \
                 JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
                 WHERE ps.process_id = $1 AND pf.template_field_id = $2 \
                 ORDER BY pf.template_field_id, pf.updated_at DESC, pf.field_instance_id DESC \
             ) cur WHERE cur.document_id = $3 \
         )",
    )
    .bind(process_id)
    .bind(template_field_id)
    .bind(document_id)
    .fetch_one(&mut *conn)
    .await
}

/// Repoint a field at a signed replacement document. Bypasses save_step/validate_save
/// since this is a system update, not a user submission.
pub async fn attach_signed_replacement(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: i32,
    template_field_id: i32,
    new_document_id: Uuid,
) -> Result<(), sqlx::Error> {
    persist_fields(
        &mut *conn,
        process_id,
        template_step_id,
        &[(template_field_id, None, Some(new_document_id))],
        None,
        None,
    )
    .await
}

/// Re-run and persist step completion. Call after a signature requirement completes
/// outside save_step/review_step.
pub async fn recompute_gating_for_step(
    conn: &mut sqlx::PgConnection,
    template: &crate::ProcessTemplate,
    process_id: Uuid,
    template_step_id: i32,
) -> Result<(), sqlx::Error> {
    let state = load_state(&mut *conn, process_id, template).await?;
    let steps_to_update =
        state_machine::compute_completion_updates(template, &state, template_step_id);
    persist_step_completions(&mut *conn, process_id, &steps_to_update).await?;
    sync_start_times(&mut *conn, process_id, template).await?;
    Ok(())
}

/// Resolve a step's instance ID from its (process_id, template_step_id) pair.
/// Shared by every write path that needs to target a specific step row.
async fn step_instance_id_for(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: i32,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT step_instance_id FROM process_steps WHERE process_id = $1 AND template_step_id = $2",
    )
    .bind(process_id)
    .bind(template_step_id)
    .fetch_one(conn)
    .await
}

async fn persist_fields(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: i32,
    fields: &[(i32, Option<String>, Option<Uuid>)], // (template_field_id, text_value, document_id)
    updated_by: Option<i32>,
    acted_as_designation: Option<&str>,
) -> Result<(), sqlx::Error> {
    if fields.is_empty() {
        return Ok(());
    }
    let step_instance_id = step_instance_id_for(&mut *conn, process_id, template_step_id).await?;

    let template_field_ids: Vec<i32> = fields.iter().map(|f| f.0).collect();
    let text_values: Vec<Option<String>> = fields.iter().map(|f| f.1.clone()).collect();
    let document_ids: Vec<Option<Uuid>> = fields.iter().map(|f| f.2).collect();

    sqlx::query(
        "INSERT INTO process_fields (step_instance_id, template_field_id, text_value, document_id, updated_by, acted_as_designation, updated_at) \
         SELECT $1, f.template_field_id, f.text_value, f.document_id, $5, $6, NOW() \
         FROM UNNEST($2::int[], $3::text[], $4::uuid[]) AS f(template_field_id, text_value, document_id)",
    )
    .bind(step_instance_id)
    .bind(&template_field_ids)
    .bind(&text_values)
    .bind(&document_ids)
    .bind(updated_by)
    .bind(acted_as_designation)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn persist_step_completions(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    steps: &[(i32, bool)], // (template_step_id, is_completed)
) -> Result<(), sqlx::Error> {
    if steps.is_empty() {
        return Ok(());
    }
    let template_step_ids: Vec<i32> = steps.iter().map(|s| s.0).collect();
    let completed_flags: Vec<bool> = steps.iter().map(|s| s.1).collect();

    sqlx::query(
        "UPDATE process_steps ps \
         SET is_completed = v.completed, \
             completed_at = CASE WHEN v.completed THEN NOW() ELSE NULL END, \
             updated_at = NOW() \
         FROM UNNEST($1::int[], $2::bool[]) AS v(template_step_id, completed) \
         WHERE ps.process_id = $3 AND ps.template_step_id = v.template_step_id",
    )
    .bind(&template_step_ids)
    .bind(&completed_flags)
    .bind(process_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn insert_review(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    params: &ReviewStepParams<'_>,
) -> Result<(), sqlx::Error> {
    let step_instance_id = step_instance_id_for(&mut *conn, process_id, params.step_id).await?;

    sqlx::query(
        "INSERT INTO process_step_reviews \
         (step_instance_id, reviewer_user_id, is_approved, notes, flagged_step_ids, acted_as_designation, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW())",
    )
    .bind(step_instance_id)
    .bind(params.reviewer_user_id)
    .bind(params.is_approved)
    .bind(params.notes)
    .bind(params.flagged_step_ids)
    .bind(params.acted_as_designation)
    .execute(&mut *conn)
    .await?;

    let event_type = if params.is_approved {
        crate::audit::AuditEventType::StepApproved
    } else {
        crate::audit::AuditEventType::StepReturned
    };
    crate::audit::insert_audit_event(
        &mut *conn,
        process_id,
        Some(params.step_id),
        event_type,
        params.notes,
        params.reviewer_user_id,
        params.acted_as_designation,
    )
    .await?;
    for &flagged_id in params.flagged_step_ids {
        crate::audit::insert_audit_event(
            &mut *conn,
            process_id,
            Some(flagged_id),
            crate::audit::AuditEventType::StepFlagged,
            None,
            params.reviewer_user_id,
            params.acted_as_designation,
        )
        .await?;
    }

    if !params.is_approved {
        pause_sla_clock(&mut *conn, process_id).await?;
    }

    Ok(())
}

/// Pauses the SLA clock (no-op if it isn't running yet, or is already paused).
async fn pause_sla_clock(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE processes SET sla_paused_at = NOW() \
         WHERE process_id = $1 AND sla_due_date IS NOT NULL AND sla_paused_at IS NULL",
    )
    .bind(process_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Resumes a paused SLA clock, folding the elapsed working-day pause into the
/// accumulated offset. No-op if the clock isn't currently paused.
async fn resume_sla_clock_if_paused(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<(), sqlx::Error> {
    let paused_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT sla_paused_at FROM processes WHERE process_id = $1")
            .bind(process_id)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
    let Some(paused_at) = paused_at else {
        return Ok(());
    };

    let holidays = get_holidays(&mut *conn).await?;
    let today = chrono::Utc::now().date_naive();
    let elapsed =
        crate::working_days::count_working_days_between(paused_at.date_naive(), today, &holidays);

    sqlx::query(
        "UPDATE processes SET sla_paused_working_days = sla_paused_working_days + $1, sla_paused_at = NULL \
         WHERE process_id = $2 AND sla_paused_at = $3",
    )
    .bind(elapsed)
    .bind(process_id)
    .bind(paused_at)
    .execute(conn)
    .await?;
    Ok(())
}

/// Starts the SLA clock the first time the checklist is fully off the applicant's
/// plate. No-op if the template has no `tat_working_days`, or the clock already ran.
async fn maybe_start_sla_clock(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    turn: state_machine::ProcessTurn,
) -> Result<(), sqlx::Error> {
    let Some(tat_working_days) = template.tat_working_days else {
        return Ok(());
    };
    if turn == state_machine::ProcessTurn::User {
        return Ok(());
    }

    let holidays = get_holidays(&mut *conn).await?;
    let today = chrono::Utc::now().date_naive();
    let due_date = crate::working_days::add_working_days(today, tat_working_days, &holidays);

    sqlx::query(
        "UPDATE processes SET tat_working_days = $1, sla_started_at = NOW(), sla_due_date = $2 \
         WHERE process_id = $3 AND sla_started_at IS NULL",
    )
    .bind(tat_working_days)
    .bind(due_date)
    .bind(process_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Every gazetted/office holiday, for working-day math.
pub async fn get_holidays(
    conn: &mut sqlx::PgConnection,
) -> Result<std::collections::HashSet<chrono::NaiveDate>, sqlx::Error> {
    let rows: Vec<(chrono::NaiveDate,)> =
        sqlx::query_as("SELECT holiday_date FROM public_holidays")
            .fetch_all(conn)
            .await?;
    Ok(rows.into_iter().map(|(d,)| d).collect())
}

/// Actual working days from SLA start to the process's `PROCESS_CLOSED` audit event,
/// net of time spent paused. `None` if the template has no SLA tracking, or the
/// process hasn't closed yet.
pub async fn compute_tat_actual(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Option<i32>, sqlx::Error> {
    let sla_started_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT sla_started_at FROM processes WHERE process_id = $1")
            .bind(process_id)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
    let Some(sla_started_at) = sla_started_at else {
        return Ok(None);
    };

    let closed_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT created_at FROM process_audit_log \
         WHERE process_id = $1 AND event_type = 'PROCESS_CLOSED' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(process_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(closed_at) = closed_at else {
        return Ok(None);
    };

    let paused_working_days: i32 =
        sqlx::query_scalar("SELECT sla_paused_working_days FROM processes WHERE process_id = $1")
            .bind(process_id)
            .fetch_one(&mut *conn)
            .await?;

    let holidays = get_holidays(&mut *conn).await?;
    let elapsed = crate::working_days::count_working_days_between(
        sla_started_at.date_naive(),
        closed_at.date_naive(),
        &holidays,
    );
    Ok(Some((elapsed - paused_working_days).max(0)))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TemplateTatSummary {
    pub template_name: String,
    pub closed_count: i32,
    pub average_tat_actual_working_days: f64,
}

/// Average `TATActual` per template, over every closed process with SLA tracking on —
/// the data behind "what's our real turnaround on NOC type X" reporting.
pub async fn tat_actual_summary_by_template(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<TemplateTatSummary>, sqlx::Error> {
    #[derive(sqlx::FromRow)]
    struct ClosedProcessSla {
        template_name: String,
        sla_started_at: chrono::DateTime<chrono::Utc>,
        sla_paused_working_days: i32,
        closed_at: chrono::DateTime<chrono::Utc>,
    }

    let rows: Vec<ClosedProcessSla> = sqlx::query_as(
        "SELECT pt.name AS template_name, p.sla_started_at, p.sla_paused_working_days, \
                a.created_at AS closed_at \
         FROM processes p \
         JOIN process_template_versions ptv ON ptv.version_id = p.template_version_id \
         JOIN process_templates pt ON pt.template_id = ptv.template_id \
         JOIN LATERAL ( \
             SELECT created_at FROM process_audit_log \
             WHERE process_id = p.process_id AND event_type = 'PROCESS_CLOSED' \
             ORDER BY created_at DESC LIMIT 1 \
         ) a ON TRUE \
         WHERE p.sla_started_at IS NOT NULL",
    )
    .fetch_all(&mut *conn)
    .await?;

    let holidays = get_holidays(&mut *conn).await?;
    let mut by_template: HashMap<String, Vec<i32>> = HashMap::new();
    for row in rows {
        let elapsed = crate::working_days::count_working_days_between(
            row.sla_started_at.date_naive(),
            row.closed_at.date_naive(),
            &holidays,
        );
        let tat_actual = (elapsed - row.sla_paused_working_days).max(0);
        by_template
            .entry(row.template_name)
            .or_default()
            .push(tat_actual);
    }

    let mut summaries: Vec<TemplateTatSummary> = by_template
        .into_iter()
        .map(|(template_name, values)| {
            let closed_count = values.len() as i32;
            let average_tat_actual_working_days =
                values.iter().sum::<i32>() as f64 / values.len() as f64;
            TemplateTatSummary {
                template_name,
                closed_count,
                average_tat_actual_working_days,
            }
        })
        .collect();
    summaries.sort_by(|a, b| a.template_name.cmp(&b.template_name));
    Ok(summaries)
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PublicHoliday {
    pub holiday_date: chrono::NaiveDate,
    pub description: String,
}

pub async fn list_public_holidays(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<PublicHoliday>, sqlx::Error> {
    sqlx::query_as("SELECT holiday_date, description FROM public_holidays ORDER BY holiday_date")
        .fetch_all(conn)
        .await
}

pub async fn add_public_holiday(
    conn: &mut sqlx::PgConnection,
    holiday_date: chrono::NaiveDate,
    description: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO public_holidays (holiday_date, description) VALUES ($1, $2) \
         ON CONFLICT (holiday_date) DO UPDATE SET description = EXCLUDED.description",
    )
    .bind(holiday_date)
    .bind(description)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn remove_public_holiday(
    conn: &mut sqlx::PgConnection,
    holiday_date: chrono::NaiveDate,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM public_holidays WHERE holiday_date = $1")
        .bind(holiday_date)
        .execute(conn)
        .await?;
    Ok(())
}

// ==========================================
// Standalone admin-only comments
// ==========================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AdminComment {
    pub comment_id: Uuid,
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub admin_email: Option<String>,
    pub comment: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Insert a standalone admin-only comment. These are never shown to residents.
pub async fn insert_admin_comment(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: i32,
    admin_id: i32,
    comment: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO process_reviewer_comments \
         (process_id, template_step_id, reviewer_user_id, comment, created_at) \
         VALUES ($1, $2, $3, $4, NOW())",
    )
    .bind(process_id)
    .bind(template_step_id)
    .bind(admin_id)
    .bind(comment)
    .execute(conn)
    .await?;

    Ok(())
}

/// Fetch all admin-only comments for a process. Only returned to admin callers.
pub async fn get_admin_comments(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<AdminComment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT ac.comment_id, ac.process_id, ac.template_step_id, \
                u.email as admin_email, ac.comment, ac.created_at \
         FROM process_reviewer_comments ac \
         LEFT JOIN users u ON ac.reviewer_user_id = u.user_id \
         WHERE ac.process_id = $1 \
         ORDER BY ac.created_at ASC",
    )
    .bind(process_id)
    .fetch_all(conn)
    .await
}
