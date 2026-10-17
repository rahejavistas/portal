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

/// Whether any step in `template` is owned by one of `designation_codes` — the scoped
/// access path a designation holder gets without needing global `process.reviewer`.
pub(super) fn designation_owns_a_step(
    template: &process::ProcessTemplate,
    designation_codes: &std::collections::HashSet<String>,
) -> bool {
    process::state_machine::build_step_info_map(template)
        .values()
        .any(|info| {
            matches!(&info.filled_by, process::StepOwner::Designation { code } if designation_codes.contains(code))
        })
}

/// Load a process by ID, verify user authorization, and load its pinned template
/// version. `precomputed_authorized` covers whatever the caller already established
/// (global reviewer, view-only oversight, a signer's own requirement, ...); this also
/// independently allows a caller holding a designation that owns one of this specific
/// process's steps, regardless of `precomputed_authorized`.
pub(super) async fn load_authorized_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    process_id: uuid::Uuid,
    precomputed_authorized: bool,
    designation_codes: &std::collections::HashSet<String>,
) -> Result<(process::db::ProcessInfo, process::ProcessTemplate), ApiError> {
    let info = process::db::get_process_info(conn, process_id)
        .await
        .internal_err()?
        .ok_or_else(|| {
            // A nonexistent process can never be owned or designation-scoped by anyone,
            // so an already-unauthorized caller learns nothing from 404 vs 403 — give
            // them the same 403 a real-but-inaccessible process would, not a signal that
            // distinguishes "doesn't exist" from "exists but isn't yours."
            if precomputed_authorized {
                (StatusCode::NOT_FOUND, Json("Process not found".to_string()))
            } else {
                (
                    StatusCode::FORBIDDEN,
                    Json("Access Denied: You do not own this process".to_string()),
                )
            }
        })?;

    let template = process::template_store::load_template_version(conn, info.template_version_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(format!("Template error: {}", e)),
            )
        })?;
    let wing_rep_code = match info.ownership_id {
        Some(oid) => process::db::get_wing_representative_code_for_ownership(conn, oid)
            .await
            .internal_err()?,
        None => None,
    };
    let template =
        process::template_store::resolve_wing_representatives(template, wing_rep_code.as_deref())
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let owns_process = info.user_id == user.user_id;
    let scoped_access = designation_owns_a_step(&template, designation_codes);
    if !precomputed_authorized && !owns_process && !scoped_access {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: You do not own this process".to_string()),
        ));
    }

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

/// Emails a designation's holders once per batch, even if a Stage and its own inherited
/// Substep both newly opened together — that's one turn, not two.
pub(super) async fn notify_newly_opened_designation_steps(
    permissions_manager: &authentication::PermissionsManager,
    template: &process::ProcessTemplate,
    process_id: uuid::Uuid,
    newly_opened_step_ids: &[i32],
) {
    let info_map = process::state_machine::build_step_info_map(template);

    let mut by_designation: std::collections::HashMap<&str, &process::state_machine::StepInfo> =
        std::collections::HashMap::new();
    for &step_id in newly_opened_step_ids {
        let Some(info) = info_map.get(&step_id) else {
            continue;
        };
        let process::StepOwner::Designation { code } = &info.filled_by else {
            continue;
        };
        let is_top_level = info.parent_id == Some(template.step_id);
        let should_replace = match by_designation.get(code.as_str()) {
            None => true,
            Some(_) => is_top_level,
        };
        if should_replace {
            by_designation.insert(code, info);
        }
    }

    for (code, info) in by_designation {
        let emails = permissions_manager
            .emails_for_designation(code)
            .await
            .unwrap_or_default();
        if emails.is_empty() {
            continue;
        }
        let label = permissions_manager
            .get_designation_label(code)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| code.to_string());
        process::notification::notify_designation_turn(
            process_id,
            &template.name,
            &info.name,
            &label,
            &emails,
        )
        .await;
    }
}

/// Catches up a newly-assigned designation holder on work that's already open and
/// waiting — a "your turn" email fires once, at the moment a step opens, so someone
/// assigned the designation afterward would otherwise never hear about it.
pub(super) async fn notify_designation_of_currently_open_steps(
    pool: &sqlx::PgPool,
    permissions_manager: &authentication::PermissionsManager,
    designation_code: &str,
    holder_email: &str,
) {
    let Ok(mut conn) = pool.acquire().await else {
        return;
    };
    let Ok(candidates) = process::db::processes_with_admin_turn(&mut conn).await else {
        return;
    };

    let label = permissions_manager
        .get_designation_label(designation_code)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| designation_code.to_string());

    let mut template_cache: std::collections::HashMap<i32, process::ProcessTemplate> =
        std::collections::HashMap::new();

    for (process_id, template_version_id, ownership_id) in candidates {
        let template = match template_cache.get(&template_version_id) {
            Some(t) => t.clone(),
            None => {
                let Ok(t) =
                    process::template_store::load_template_version(&mut conn, template_version_id)
                        .await
                else {
                    continue;
                };
                template_cache.insert(template_version_id, t.clone());
                t
            }
        };
        let wing_rep_code = match ownership_id {
            Some(oid) => process::db::get_wing_representative_code_for_ownership(&mut conn, oid)
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let Ok(template) = process::template_store::resolve_wing_representatives(
            template,
            wing_rep_code.as_deref(),
        ) else {
            continue;
        };

        let Ok(steps) = process::db::load_steps(&mut conn, process_id).await else {
            continue;
        };
        let reviews = process::db::get_process_reviews(&mut conn, process_id)
            .await
            .unwrap_or_default();
        let mut reviews_by_step: std::collections::HashMap<
            i32,
            Vec<process::state_machine::ReviewState>,
        > = std::collections::HashMap::new();
        for r in &reviews {
            reviews_by_step.entry(r.template_step_id).or_default().push(
                process::state_machine::ReviewState {
                    is_approved: r.is_approved,
                    created_at: r.created_at,
                },
            );
        }
        let state_input = process::state_machine::ProcessStateInput {
            steps,
            reviews: reviews_by_step,
            ..Default::default()
        };
        let statuses = process::state_machine::compute_statuses(&template, &state_input);
        let info_map = process::state_machine::build_step_info_map(&template);

        let mut matching: Option<&process::state_machine::StepInfo> = None;
        for (step_id, status) in &statuses {
            if *status != process::state_machine::StepStatus::Open {
                continue;
            }
            let Some(info) = info_map.get(step_id) else {
                continue;
            };
            let process::StepOwner::Designation { code } = &info.filled_by else {
                continue;
            };
            if code != designation_code {
                continue;
            }
            let is_top_level = info.parent_id == Some(template.step_id);
            if matching.is_none() || is_top_level {
                matching = Some(info);
            }
        }

        if let Some(info) = matching {
            process::notification::notify_designation_turn(
                process_id,
                &template.name,
                &info.name,
                &label,
                &[holder_email.to_string()],
            )
            .await;
        }
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
