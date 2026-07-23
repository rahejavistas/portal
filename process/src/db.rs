use std::collections::HashMap;
use uuid::Uuid;

use crate::state_machine::{
    self, FieldState, ProcessStateInput, ReviewState, StepStateDb, StepStatus,
};

// ==========================================
// Top-level process operations
// ==========================================

/// Create a new process and all its step rows. Returns the process_id.
pub async fn init_process(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
    template_version_id: i32,
    template: &crate::ProcessTemplate,
    ownership_id: Option<i32>,
) -> Result<Uuid, sqlx::Error> {
    let process_id: Uuid = sqlx::query_scalar(
        "INSERT INTO processes (user_id, template_version_id, ownership_id) \
         VALUES ($1, $2, $3) RETURNING process_id",
    )
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

    Ok(process_id)
}

/// Outcome of a successful `save_step` call.
pub struct SaveStepOutcome {
    pub status: StepStatus,
}

/// Save fields for a step. Validates, persists fields, and updates completion.
/// Returns the new step status.
pub async fn save_step(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    step_id: i32,
    fields: &[state_machine::SaveFieldInput],
    is_admin: bool,
) -> Result<SaveStepOutcome, String> {
    let state = load_state(conn, process_id)
        .await
        .map_err(|e| e.to_string())?;
    let result = state_machine::validate_save(template, &state, step_id, fields, is_admin)?;

    persist_fields(&mut *conn, process_id, step_id, &result.fields_to_save)
        .await
        .map_err(|e| e.to_string())?;

    let steps_to_update = result.steps_to_update;
    let status = steps_to_update
        .iter()
        .find(|(sid, _)| *sid == step_id)
        .map(|(_, completed)| {
            if *completed {
                if state_machine::build_step_info_map(template)
                    .get(&step_id)
                    .map(|i| i.is_review_required)
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

    sync_start_times(&mut *conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;

    Ok(SaveStepOutcome { status })
}

/// Parameters for recording an admin review.
/// Bundled into a struct to keep `review_step` within clippy argument limits.
pub struct ReviewStepParams<'a> {
    pub step_id: i32,
    pub reviewer_user_id: Option<i32>,
    pub is_approved: bool,
    pub notes: Option<&'a str>,
    /// Template step IDs (= `process_steps.template_step_id`) the admin flagged
    /// as needing resident attention. Purely informational — does not affect the
    /// state machine. See the migration comment for full details.
    pub flagged_step_ids: &'a [i32],
}

/// Record an admin review (approve/reject).
/// Returns whether the step is now completed (`process_steps.is_completed`).
pub async fn review_step(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    params: ReviewStepParams<'_>,
) -> Result<bool, String> {
    let state = load_state(conn, process_id)
        .await
        .map_err(|e| e.to_string())?;
    let (steps_to_update, is_completed) = state_machine::compute_review_updates(
        template,
        &state,
        params.step_id,
        params.is_approved,
    )?;

    persist_step_completions(&mut *conn, process_id, &steps_to_update)
        .await
        .map_err(|e| e.to_string())?;

    insert_review(&mut *conn, process_id, &params)
        .await
        .map_err(|e| e.to_string())?;

    sync_start_times(&mut *conn, process_id, template)
        .await
        .map_err(|e| e.to_string())?;

    Ok(is_completed)
}

/// Reopen a completed/pending-review step by inserting rejection reviews and resetting completion.
pub async fn reopen_step(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
    step_id: i32,          // template_step_id of the step to reopen
    reviewer_user_id: i32, // users.user_id of the reviewer performing the reopen
    custom_note: Option<&str>,
) -> Result<(), String> {
    let state = load_state(conn, process_id)
        .await
        .map_err(|e| e.to_string())?;
    let steps_to_reset = state_machine::compute_reopen_updates(template, &state, step_id)?;
    let statuses = state_machine::compute_statuses(template, &state);

    // Insert rejection reviews for steps that are currently closed
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

    Ok(())
}

/// Compute statuses for all steps in a process.
pub async fn get_statuses(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<HashMap<i32, StepStatus>, String> {
    let state = load_state(conn, process_id)
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
    let state = load_state(conn, process_id).await?;
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
}

pub async fn get_process_info(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Option<ProcessInfo>, sqlx::Error> {
    sqlx::query_as(
        "SELECT user_id, template_version_id, ownership_id \
         FROM processes WHERE process_id = $1",
    )
    .bind(process_id)
    .fetch_optional(conn)
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

#[derive(serde::Serialize, sqlx::FromRow)]
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
}

pub async fn get_process_reviews(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<ProcessReviewHistory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT pr.review_id, ps.template_step_id, u.email as admin_email, \
                pr.is_approved, pr.notes, pr.created_at, pr.flagged_step_ids \
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
}

pub async fn get_field_history(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<ProcessFieldHistoryRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT pf.template_field_id, pf.text_value, pf.document_id, pf.updated_at \
         FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
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
    pub template_version_id: i32,
    pub template_name: String,
    pub ownership_id: Option<i32>,
    pub flat_label: Option<String>,
    pub user_email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub steps_total: i64,
    pub steps_completed: i64,
    #[sqlx(default)]
    pub status: Option<String>,
}

const PROCESS_LIST_QUERY_BASE: &str =
    "SELECT p.process_id, p.template_version_id, pt.name AS template_name, p.ownership_id, \
            CASE WHEN fo.flat_id IS NOT NULL \
                 THEN f.wing || '-' || f.flat_number \
                 ELSE NULL END AS flat_label, \
            u.email AS user_email, \
            p.created_at, \
            COUNT(ps.step_instance_id) FILTER (WHERE ps.parent_step_instance_id IS NOT NULL) AS steps_total, \
            COUNT(ps.step_instance_id) FILTER (WHERE ps.parent_step_instance_id IS NOT NULL AND ps.is_completed = TRUE) AS steps_completed \
     FROM processes p \
     JOIN users u ON p.user_id = u.user_id \
     JOIN process_template_versions ptv ON ptv.version_id = p.template_version_id \
     JOIN process_templates pt ON pt.template_id = ptv.template_id \
     LEFT JOIN flat_ownerships fo ON p.ownership_id = fo.ownership_id \
     LEFT JOIN flats f ON fo.flat_id = f.flat_id \
     LEFT JOIN process_steps ps ON p.process_id = ps.process_id";

pub async fn list_processes_for_user(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
) -> Result<Vec<ProcessListItem>, sqlx::Error> {
    let query = format!(
        "{} WHERE p.user_id = $1 AND (fo.ownership_id IS NULL OR fo.is_active = TRUE) \
         GROUP BY p.process_id, p.template_version_id, pt.name, p.ownership_id, fo.flat_id, f.wing, f.flat_number, u.email, p.created_at \
         ORDER BY p.created_at DESC",
        PROCESS_LIST_QUERY_BASE
    );
    sqlx::query_as(&query).bind(user_id).fetch_all(conn).await
}

pub async fn list_all_processes(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<ProcessListItem>, sqlx::Error> {
    let query = format!(
        "{} \
         GROUP BY p.process_id, p.template_version_id, pt.name, p.ownership_id, fo.flat_id, f.wing, f.flat_number, u.email, p.created_at \
         ORDER BY p.created_at DESC",
        PROCESS_LIST_QUERY_BASE
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

/// Load per-step completion state (is_completed, started_at) for a process.
/// Split out from `load_state` so callers that already fetch full field/review
/// history elsewhere (e.g. the process-state read endpoint) can derive the rest
/// of `ProcessStateInput` from data they have instead of re-querying it.
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

    Ok(ProcessStateInput {
        steps,
        fields,
        reviews,
    })
}

/// Set started_at for any step that is non-Locked but hasn't been started yet.
async fn sync_start_times(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template: &crate::ProcessTemplate,
) -> Result<(), sqlx::Error> {
    let state = load_state(&mut *conn, process_id).await?;
    let statuses = state_machine::compute_statuses(template, &state);
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
) -> Result<(), sqlx::Error> {
    let step_instance_id = step_instance_id_for(&mut *conn, process_id, template_step_id).await?;

    for &(template_field_id, ref text_value, document_id) in fields {
        sqlx::query(
            "INSERT INTO process_fields (step_instance_id, template_field_id, text_value, document_id, updated_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(step_instance_id)
        .bind(template_field_id)
        .bind(text_value)
        .bind(document_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn persist_step_completions(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    steps: &[(i32, bool)], // (template_step_id, is_completed)
) -> Result<(), sqlx::Error> {
    for &(template_step_id, is_completed) in steps {
        sqlx::query(
            "UPDATE process_steps \
             SET is_completed = $1, completed_at = CASE WHEN $1 = TRUE THEN NOW() ELSE NULL END, updated_at = NOW() \
             WHERE process_id = $2 AND template_step_id = $3",
        )
        .bind(is_completed)
        .bind(process_id)
        .bind(template_step_id)
        .execute(&mut *conn)
        .await?;
    }
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
         (step_instance_id, reviewer_user_id, is_approved, notes, flagged_step_ids, created_at) \
         VALUES ($1, $2, $3, $4, $5, NOW())",
    )
    .bind(step_instance_id)
    .bind(params.reviewer_user_id)
    .bind(params.is_approved)
    .bind(params.notes)
    .bind(params.flagged_step_ids)
    .execute(&mut *conn)
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
