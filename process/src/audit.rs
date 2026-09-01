use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuditEventType {
    StepCompleted,
    StepApproved,
    StepReturned,
    StepFlagged,
    /// The whole process reached its terminal state (root step completed) — the
    /// handover moment `TATActual` is measured against.
    ProcessClosed,
}

impl AuditEventType {
    fn as_db_str(&self) -> &'static str {
        match self {
            AuditEventType::StepCompleted => "STEP_COMPLETED",
            AuditEventType::StepApproved => "STEP_APPROVED",
            AuditEventType::StepReturned => "STEP_RETURNED",
            AuditEventType::StepFlagged => "STEP_FLAGGED",
            AuditEventType::ProcessClosed => "PROCESS_CLOSED",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "STEP_COMPLETED" => Some(AuditEventType::StepCompleted),
            "STEP_APPROVED" => Some(AuditEventType::StepApproved),
            "STEP_RETURNED" => Some(AuditEventType::StepReturned),
            "STEP_FLAGGED" => Some(AuditEventType::StepFlagged),
            "PROCESS_CLOSED" => Some(AuditEventType::ProcessClosed),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AuditEventType::StepCompleted => "Submitted",
            AuditEventType::StepApproved => "Approved",
            AuditEventType::StepReturned => "Returned",
            AuditEventType::StepFlagged => "Flagged for attention",
            AuditEventType::ProcessClosed => "Process closed",
        }
    }
}

/// Appends one row. Nothing in this crate ever updates or deletes from
/// `process_audit_log` — a correction is a new row, not an edit to this one.
pub async fn insert_audit_event(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: Option<i32>,
    event_type: AuditEventType,
    detail: Option<&str>,
    actor_id: Option<i32>,
    acted_as_designation: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO process_audit_log (process_id, template_step_id, event_type, detail, actor_id, acted_as_designation) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(process_id)
    .bind(template_step_id)
    .bind(event_type.as_db_str())
    .bind(detail)
    .bind(actor_id)
    .bind(acted_as_designation)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuditLogEntry {
    pub audit_id: i64,
    pub template_step_id: Option<i32>,
    pub event_type: String,
    pub detail: Option<String>,
    pub actor_email: Option<String>,
    pub acted_as_designation: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn get_audit_log(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<AuditLogEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.audit_id, a.template_step_id, a.event_type, a.detail, u.email AS actor_email, \
                a.acted_as_designation, a.created_at \
         FROM process_audit_log a \
         LEFT JOIN users u ON u.user_id = a.actor_id \
         WHERE a.process_id = $1 \
         ORDER BY a.created_at ASC, a.audit_id ASC",
    )
    .bind(process_id)
    .fetch_all(conn)
    .await
}
