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
