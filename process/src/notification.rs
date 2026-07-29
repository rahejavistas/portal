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

// ---------------------------------------------------------------------------
// Internal dispatch helper — every public function delegates here.
// Failures are logged but never propagated, so callers never gate business
// logic on notification delivery.
// ---------------------------------------------------------------------------

async fn dispatch(to: &str, subject: String, body: String, process_id: Uuid) {
    let payload = email::Email::Generic {
        to: to.to_string(),
        subject,
        body,
    };

    if let Err(e) = email::send_email(&payload).await {
        eprintln!(
            "[NOTIFICATION] Email failed for process {}: {}",
            process_id, e
        );
    }
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
    let subject = format!("[{}] Document Awaiting Your Signature", process_name);
    let body = format!(
        "Hello,\n\n\
         A document requires your signature.\n\n\
         Document: {}\n\
         Step: {}\n\
         Process: {}\n\
         Process ID: {}\n\n\
         Please log in to the Raheja Vistas portal to review and sign.\n",
        document_title, step_name, process_name, process_id,
    );

    dispatch(signer_email, subject, body, process_id).await;
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
    let subject = format!("[{}] It's Your Turn to Sign", process_name);
    let body = format!(
        "Hello,\n\n\
         The signer(s) ahead of you have finished, and it's now your turn to sign.\n\n\
         Document: {}\n\
         Step: {}\n\
         Process: {}\n\
         Process ID: {}\n\n\
         Please log in to the Raheja Vistas portal to review and sign.\n",
        document_title, step_name, process_name, process_id,
    );

    dispatch(signer_email, subject, body, process_id).await;
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
    let subject = format!("[{}] Step Completed", ctx.process_name);
    let body = format!(
        "Hello,\n\n\
         The last outstanding signature on \"{}\" has been completed, and the step is now done.\n\n\
         Process: {}\n\
         Process ID: {}\n\n\
         Please log in to the Raheja Vistas portal for details.\n",
        step_name, ctx.process_name, process_id,
    );

    dispatch(&ctx.user_email, subject, body, process_id).await;
}

/// Notify admin that a step has been submitted and is pending review.
pub async fn notify_admin_pending_review(
    ctx: &NotificationContext,
    process_id: Uuid,
    step_name: &str,
) {
    dispatch(
        &ctx.admin_email,
        format!("[{}] Review Required – {}", ctx.process_name, step_name),
        format!(
            "Hello,\n\n\
             Step \"{}\" in process \"{}\" has been submitted for review.\n\n\
             Resident: {}\n\
             Process ID: {}\n\n\
             Please log in to the Raheja Vistas portal to review.\n",
            step_name, ctx.process_name, ctx.user_email, process_id,
        ),
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
    let (verdict, subject) = if params.is_approved {
        (
            "APPROVED",
            format!(
                "[{}] ✓ Step Approved – {}",
                ctx.process_name, params.step_name
            ),
        )
    } else {
        (
            "REVISION REQUIRED",
            format!(
                "[{}] ↩ Revision Required – {}",
                ctx.process_name, params.step_name
            ),
        )
    };

    let notes_section = match params.notes {
        Some(n) if !n.trim().is_empty() => format!(
            "\nAdmin Comments:\n──────────────────────────────\n{}\n──────────────────────────────\n",
            n
        ),
        _ => String::new(),
    };

    let flagged_section = if !params.flagged_step_names.is_empty() {
        let items: Vec<String> = params
            .flagged_step_names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("  {}. {}", i + 1, name))
            .collect();
        format!("\nFlagged sub-steps:\n{}\n", items.join("\n"))
    } else {
        String::new()
    };

    dispatch(
        &ctx.user_email,
        subject,
        format!(
            "Dear Resident,\n\n\
             Your submission for step \"{}\" has been reviewed: {verdict}\n\n\
             Process: {}\n\
             Process ID: {}\n\
             {}{}\n\
             Please log in to the Raheja Vistas portal to view your process.\n",
            params.step_name, ctx.process_name, params.process_id, notes_section, flagged_section,
        ),
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
    let notes_section = match notes {
        Some(n) if !n.trim().is_empty() => format!(
            "\n\nAdmin Comments:\n──────────────────────────────\n{}\n──────────────────────────────",
            n
        ),
        _ => String::new(),
    };

    dispatch(
        &ctx.user_email,
        format!("[{}] Step Reopened – {}", ctx.process_name, step_name),
        format!(
            "Dear Resident,\n\n\
             Step \"{}\" in process \"{}\" has been reopened by an administrator. \
             Additional action may be required.{}\n\n\
             Process ID: {}\n\n\
             Please log in to the Raheja Vistas portal to continue.\n",
            step_name, ctx.process_name, notes_section, process_id,
        ),
        process_id,
    )
    .await;
}
