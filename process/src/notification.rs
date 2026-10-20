use uuid::Uuid;

/// Context for dispatching a notification without needing DB access.
pub struct NotificationContext {
    /// Email address of the resident who owns the process.
    pub user_email: String,
    /// Email address of the site administrator (from app config).
    pub admin_email: String,
    /// Human-readable process name (e.g. "Tenant Onboarding").
    pub process_name: String,
}

const PROCESS_UPDATE_SUBJECT: &str = "Update to a process";

// Must stay awaited, never spawned off-task — this also runs as a Lambda, which can freeze right after responding.
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
        attachment: None,
    };

    if let Err(e) = email::send_email(&payload).await {
        eprintln!(
            "[NOTIFICATION] Email failed for process {}: {}",
            process_id, e
        );
    }
}

/// Deep-links into the resident's own process detail.
fn portal_process_url(process_id: Uuid) -> String {
    format!(
        "{}/process?process_id={}",
        email::config::portal_base_url(),
        process_id
    )
}

/// Deep-links a reviewer into the process needing review.
fn portal_admin_process_url(process_id: Uuid) -> String {
    format!(
        "{}/admin-process?process_id={}",
        email::config::portal_base_url(),
        process_id
    )
}

/// Deep-links a signer into their signing queue.
fn portal_signature_queue_url() -> String {
    format!("{}/admin?tab=signatures", email::config::portal_base_url())
}

// Only cross-role transitions get an email: submit->review, approve/reject, revoke.

/// Notify a signer a document was attached to them for signature.
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
            format!("Step: {} - Process: {}", step_name, process_name),
        ],
        portal_signature_queue_url(),
        process_id,
    )
    .await;
}

/// Notify a signer it's now their turn. Manual-flow only (SignYu notifies its own).
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
                "The signer(s) ahead of you on \"{}\" have finished - it's now your turn.",
                document_title
            ),
            format!("Step: {} - Process: {}", step_name, process_name),
        ],
        portal_signature_queue_url(),
        process_id,
    )
    .await;
}

/// Notify the resident a step completed via its last outstanding signature. Only call
/// once the step has actually transitioned to Completed.
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

/// Notify every active holder of a designation that a stage assigned to it just opened —
/// only for `Designation`-owned stages, not the generic "any admin" case.
pub async fn notify_designation_turn(
    process_id: Uuid,
    process_name: &str,
    step_name: &str,
    designation_label: &str,
    recipient_emails: &[String],
) {
    for recipient in recipient_emails {
        dispatch(
            recipient,
            "Your Turn on a Process",
            vec![format!(
                "Stage \"{}\" in process \"{}\" is now waiting on {}.",
                step_name, process_name, designation_label
            )],
            portal_admin_process_url(process_id),
            process_id,
        )
        .await;
    }
}

/// Notify the flat's current tenant their `Tenant`-owned stage just opened.
pub async fn notify_tenant_turn(
    process_id: Uuid,
    process_name: &str,
    step_name: &str,
    tenant_email: &str,
) {
    dispatch(
        tenant_email,
        "Your Turn on a Process",
        vec![format!(
            "Stage \"{}\" in process \"{}\" is now waiting on you.",
            step_name, process_name
        )],
        portal_process_url(process_id),
        process_id,
    )
    .await;
}

/// Notify the flat's owner their `User`-owned stage just opened.
pub async fn notify_owner_turn(
    process_id: Uuid,
    process_name: &str,
    step_name: &str,
    owner_email: &str,
) {
    dispatch(
        owner_email,
        "Your Turn on a Process",
        vec![format!(
            "Stage \"{}\" in process \"{}\" is now waiting on you.",
            step_name, process_name
        )],
        portal_process_url(process_id),
        process_id,
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
