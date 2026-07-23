//! Process-instance endpoints, split by concern: `state` (read the process tree),
//! `actions` (save/review/revoke/note write-transitions), `uploads` (document
//! upload/download URLs for a process). Template AUTHORING (the versioned builder
//! API) is a separate concern entirely — see `handlers::process_templates`.

mod actions;
mod state;
mod uploads;

pub use actions::*;
pub use state::*;
pub use uploads::*;

use axum::{http::StatusCode, Json};

pub(super) type ApiError = (StatusCode, Json<String>);

/// Shorthand for the extremely common "DB/internal error -> 500" mapping,
/// cutting the repeated `.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))`
/// closure down to a single call.
pub(super) trait ResultExt<T> {
    fn internal_err(self) -> Result<T, ApiError>;
}

impl<T, E: ToString> ResultExt<T> for Result<T, E> {
    fn internal_err(self) -> Result<T, ApiError> {
        self.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))
    }
}

/// Load a process by ID, verify user authorization, and load its pinned template
/// version. Shared across save-step, review, revoke, note, and upload handlers.
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

/// Look up a step's display name from the template. Falls back to a generic
/// label if the ID isn't in the template (shouldn't happen in practice).
pub(super) fn find_step_name(template: &process::ProcessTemplate, step_id: i32) -> String {
    process::state_machine::build_step_info_map(template)
        .get(&step_id)
        .map(|info| info.name.clone())
        .unwrap_or_else(|| format!("Step {}", step_id))
}

/// Build a `NotificationContext` for email dispatch.
/// Looks up the process owner's email from the DB. Returns `None` if
/// the lookup fails (notification will be skipped gracefully).
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
