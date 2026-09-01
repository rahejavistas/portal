//! Process-instance endpoints: `state` (read), `actions` (write-transitions),
//! `uploads` (document URLs). Template authoring lives in `handlers::process_templates`.

mod actions;
mod state;
mod uploads;

pub use actions::*;
pub use state::*;
pub use uploads::*;

use axum::{http::StatusCode, Json};

pub(super) use crate::handlers::error::{ApiError, ResultExt};

/// Load a process by ID, verify user authorization, and load its pinned template
/// version.
pub(super) async fn load_authorized_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    process_id: uuid::Uuid,
    is_admin: bool,
) -> Result<(process::db::ProcessInfo, process::ProcessTemplate), ApiError> {
    let info = process::db::get_process_info(conn, process_id)
        .await
        .internal_err()?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json("Process not found".to_string())))?;

    if !is_admin && info.user_id != user.user_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: You do not own this process".to_string()),
        ));
    }

    let template = process::template_store::load_template_version(conn, info.template_version_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(format!("Template error: {}", e)),
            )
        })?;

    Ok((info, template))
}

/// Whether this user has admin-level read access to every resident's processes —
/// `process.reviewer` or `process.view`, the same gate `/admin-process` itself requires.
pub(super) fn can_view_all_processes(user: &authentication::User) -> bool {
    user.has_permission(authentication::Permission::ProcessReviewer)
        || user.has_permission(authentication::Permission::ProcessView)
}

/// Resolved fresh on every call — never cached.
pub(super) async fn build_actor_context(
    permissions_manager: &authentication::PermissionsManager,
    user: &authentication::User,
) -> process::state_machine::ActorContext {
    let designation_codes = permissions_manager
        .active_designation_codes(user.user_id)
        .await
        .unwrap_or_default();
    process::state_machine::ActorContext {
        is_admin: user.has_permission(authentication::Permission::ProcessReviewer),
        designation_codes,
    }
}

/// Emails a designation's holders for each newly-opened step it owns.
pub(super) async fn notify_newly_opened_designation_steps(
    permissions_manager: &authentication::PermissionsManager,
    template: &process::ProcessTemplate,
    process_id: uuid::Uuid,
    newly_opened_step_ids: &[i32],
) {
    let info_map = process::state_machine::build_step_info_map(template);
    for &step_id in newly_opened_step_ids {
        let Some(info) = info_map.get(&step_id) else {
            continue;
        };
        let process::StepOwner::Designation { code } = &info.filled_by else {
            continue;
        };
        let emails = permissions_manager
            .emails_for_designation(code)
            .await
            .unwrap_or_default();
        if emails.is_empty() {
            continue;
        }
        process::notification::notify_designation_turn(
            process_id,
            &template.name,
            &info.name,
            code,
            &emails,
        )
        .await;
    }
}

pub(super) fn find_step_name(template: &process::ProcessTemplate, step_id: i32) -> String {
    process::state_machine::build_step_info_map(template)
        .get(&step_id)
        .map(|info| info.name.clone())
        .unwrap_or_else(|| format!("Step {}", step_id))
}

/// Builds a NotificationContext, looking up the owner's email. None if the lookup fails.
pub(super) async fn build_notification_ctx(
    pool: &sqlx::PgPool,
    owner_user_id: i32,
    admin_email: &str,
    process_name: &str,
) -> Option<process::notification::NotificationContext> {
    let mut conn = pool.acquire().await.ok()?;
    let user_email = process::db::get_user_email(&mut conn, owner_user_id)
        .await
        .ok()
        .flatten()?;
    Some(process::notification::NotificationContext {
        user_email,
        admin_email: admin_email.to_string(),
        process_name: process_name.to_string(),
    })
}
