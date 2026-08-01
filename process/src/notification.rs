use uuid::Uuid;

/// Context passed into notification functions so they can dispatch both
/// log entries and email notifications without needing database access.
pub struct NotificationContext {
    /// Email address of the resident who owns the process.
    pub user_email: String,
    /// Email address of the site administrator (from app config).
    pub admin_email: String,
    /// Human-readable process name (e.g. "Tenant Onboarding").
    pub process_name: String,
}

const PROCESS_UPDATE_SUBJECT: &str = "Update to a process";

// ---------------------------------------------------------------------------
// Internal dispatch helper — every public function delegates here.
// Failures are logged but never propagated, so callers never gate business
// logic on notification delivery.
// ---------------------------------------------------------------------------

async fn dispatch(
    to: &str,
    heading: &str,
    paragraphs: Vec<String>,
    cta_url: String,
    process_id: Uuid,
) {
    let payload = email::EmailMessage {
        to: to.to_string(),
        subject: PROCESS_UPDATE_SUBJECT.to_string(),
        heading: heading.to_string(),
        paragraphs,
        code: None,
        cta: Some(email::Cta {
            label: "Open Portal".to_string(),
            url: cta_url,
        }),
    };

    if let Err(e) = email::send_email(&payload).await {
        eprintln!(
            "[NOTIFICATION] Email failed for process {}: {}",
            process_id, e
        );
    }
}

/// Deep-links straight into the resident's own process detail (the resident-facing
/// `/process` page reads `?process_id=` on load and opens that process directly).
fn portal_process_url(process_id: Uuid) -> String {
    format!(
        "{}/process?process_id={}",
        email::config::portal_base_url(),
        process_id
    )
}

/// Deep-links a reviewer straight into the process needing review (the `/admin-process`
/// page shares the same `?process_id=` deep-link support as the resident-facing one).
fn portal_admin_process_url(process_id: Uuid) -> String {
    format!(
        "{}/admin-process?process_id={}",
        email::config::portal_base_url(),
        process_id
    )
}

/// Deep-links a signer straight into their signing queue (the `/admin` page reads
/// `?tab=` on load — `signatures` is open to any designation holder, not just admins).
fn portal_signature_queue_url() -> String {
    format!("{}/admin?tab=signatures", email::config::portal_base_url())
}

// ---------------------------------------------------------------------------
// Public notification functions — only meaningful state transitions.
//
// We deliberately do NOT email users on partial saves or self-triggered
// completions. Emails are reserved for cross-role transitions:
//   1. User submits → admin needs to review
//   2. Admin approves/rejects → user needs to know
//   3. Admin revokes/reopens → user needs to act
// ---------------------------------------------------------------------------

/// Notify a signer that a document was just attached to them for signature. Unlike the
/// other functions here, the recipient is a specific signer (not derived from
/// `NotificationContext`), so it's passed in directly — this function stays DB-free
/// like the rest of this module.
pub async fn notify_signature_queue(
    signer_email: &str,
    process_name: &str,
    step_name: &str,
    document_title: &str,
    process_id: Uuid,
) {
    dispatch(
        signer_email,
        "Document Awaiting Your Signature",
        vec![
            format!("\"{}\" requires your signature.", document_title),
            format!("Step: {} — Process: {}", step_name, process_name),
        ],
        portal_signature_queue_url(),
        process_id,
    )
    .await;
}

/// Notify a signer that it's now their turn — an earlier signer in the chain just
/// completed, unblocking them. Manual-flow only, same as `notify_signature_queue`
/// (SignYu manages its own signer-to-signer notifications).
pub async fn notify_signature_turn(
    signer_email: &str,
    process_name: &str,
    step_name: &str,
    document_title: &str,
    process_id: Uuid,
) {
    dispatch(
        signer_email,
        "It's Your Turn to Sign",
        vec![
            format!(
                "The signer(s) ahead of you on \"{}\" have finished — it's now your turn.",
                document_title
            ),
            format!("Step: {} — Process: {}", step_name, process_name),
        ],
        portal_signature_queue_url(),
        process_id,
    )
    .await;
}

/// Notify the resident that a step just completed as a direct result of its last
/// outstanding signature finishing — NOT sent merely because a signature chain
/// finished (that's an internal sub-state of the step, not a cross-role transition on
/// its own). Only call this once the step's status has actually transitioned to
/// Completed.
pub async fn notify_signature_step_complete(
    ctx: &NotificationContext,
    process_id: Uuid,
    step_name: &str,
) {
    dispatch(
        &ctx.user_email,
        "Step Completed",
        vec![format!(
            "The last outstanding signature on \"{}\" has been completed, and the step is now done.",
            step_name
        ), format!("Process: {}", ctx.process_name)],
        portal_process_url(process_id),
        process_id,
    )
    .await;
}

/// Notify admin that a step has been submitted and is pending review.
pub async fn notify_admin_pending_review(
    ctx: &NotificationContext,
    process_id: Uuid,
    step_name: &str,
) {
    dispatch(
        &ctx.admin_email,
        "Review Required",
        vec![
            format!(
                "Step \"{}\" in process \"{}\" has been submitted for review.",
                step_name, ctx.process_name
            ),
            format!("Resident: {}", ctx.user_email),
        ],
        portal_admin_process_url(process_id),
        process_id,
    )
    .await;
}

pub struct ReviewNotificationParams<'a> {
    pub process_id: Uuid,
    pub step_name: &'a str,
    pub is_approved: bool,
    pub notes: Option<&'a str>,
    pub flagged_step_names: &'a [String],
}

/// Notify the user about an admin review decision (approval or rejection).
pub async fn notify_user_review(ctx: &NotificationContext, params: ReviewNotificationParams<'_>) {
    let heading = if params.is_approved {
        "Step Approved"
    } else {
        "Revision Required"
    };

    let mut paragraphs = vec![format!(
        "Your submission for step \"{}\" in \"{}\" has been reviewed: {}.",
        params.step_name,
        ctx.process_name,
        if params.is_approved {
            "APPROVED"
        } else {
            "REVISION REQUIRED"
        }
    )];

    if let Some(n) = params.notes {
        if !n.trim().is_empty() {
            paragraphs.push(format!("Admin comments: {}", n));
        }
    }

    if !params.flagged_step_names.is_empty() {
        paragraphs.push(format!(
            "Flagged sub-steps: {}",
            params.flagged_step_names.join(", ")
        ));
    }

    dispatch(
        &ctx.user_email,
        heading,
        paragraphs,
        portal_process_url(params.process_id),
        params.process_id,
    )
    .await;
}

/// Notify the user that an admin has reopened a step.
pub async fn notify_user_revoke(
    ctx: &NotificationContext,
    process_id: Uuid,
    step_name: &str,
    notes: Option<&str>,
) {
    let mut paragraphs = vec![format!(
        "Step \"{}\" in process \"{}\" has been reopened by an administrator. Additional action may be required.",
        step_name, ctx.process_name
    )];

    if let Some(n) = notes {
        if !n.trim().is_empty() {
            paragraphs.push(format!("Admin comments: {}", n));
        }
    }

    dispatch(
        &ctx.user_email,
        "Step Reopened",
        paragraphs,
        portal_process_url(process_id),
        process_id,
    )
    .await;
}
