//! Write-path process transitions: saving a step's fields, admin review/revoke,
//! and standalone admin notes.

use super::{build_notification_ctx, find_step_name, load_authorized_process, ApiError, ResultExt};
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct SaveStepRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub fields: Vec<SaveStepFieldInput>,
}

#[derive(Deserialize)]
pub struct SaveStepFieldInput {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct SaveStepResponse {
    pub status: String,
    pub message: String,
    pub step: SavedStepInfo,
}

#[derive(Serialize)]
pub struct SavedStepInfo {
    pub template_step_id: i32,
    pub is_completed: bool,
}

#[derive(Deserialize)]
pub struct AdminReviewRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub is_approved: bool,
    pub notes: Option<String>,
    /// Optional list of substep template_step_ids the admin is flagging for
    /// resident attention. Stored as an annotation on the review record.
    pub flagged_step_ids: Option<Vec<i32>>,
}

#[derive(Serialize)]
pub struct AdminReviewResponse {
    pub status: String,
    pub message: String,
    pub is_completed: bool,
}

#[derive(Deserialize)]
pub struct AdminRevokeRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct AdminRevokeResponse {
    pub status: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AdminAddNoteRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub comment: String,
}

#[derive(Serialize)]
pub struct AdminAddNoteResponse {
    pub status: String,
    pub message: String,
}

/// POST /api/v1/process/save-step
pub async fn save_process_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SaveStepRequest>,
) -> Result<Json<SaveStepResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.internal_err()?;

    // Write path: deliberately NOT widened to process.view — that permission is read-only
    // oversight and must never gain the ability to save a step on someone else's process.
    let is_admin = user.has_permission(authentication::Permission::ProcessReviewer);
    let (info, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, is_admin).await?;

    // Validate document ownership and upload confirmation
    for f in &payload.fields {
        if let Some(doc_id) = f.document_id {
            // Check if document is valid AND the user has permission to access it
            let doc_opt = state
                .document_manager
                .get_document(doc_id, &user)
                .await
                .internal_err()?;

            match doc_opt {
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(format!(
                            "Document ID {} is invalid or you do not have permission to access it",
                            doc_id
                        )),
                    ));
                }
                Some(ref doc) if !doc.document.is_upload_confirmed => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(format!(
                            "Document ID {} has not been uploaded yet. Please complete the upload first.",
                            doc_id
                        )),
                    ));
                }
                // Process fields only ever hold flat-scoped documents. Other categories
                // are visible in a user's document list but must never be attachable to a
                // process step.
                Some(ref doc)
                    if doc.document.category != crate::document::RESIDENT_SPECIFIC_CATEGORY =>
                {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(format!(
                            "Document ID {} is not a Resident Specific document and cannot be attached to a process step",
                            doc_id
                        )),
                    ));
                }
                _ => {}
            }
        }
    }

    let fields_submitted: Vec<process::state_machine::SaveFieldInput> = payload
        .fields
        .into_iter()
        .map(|f| process::state_machine::SaveFieldInput {
            template_field_id: f.template_field_id,
            text_value: f.text_value,
            document_id: f.document_id,
        })
        .collect();

    let save_outcome = process::db::save_step(
        &mut tx,
        payload.process_id,
        &template,
        payload.template_step_id,
        &fields_submitted,
        is_admin,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;
    let step_status = save_outcome.status;

    let is_completed = step_status == process::state_machine::StepStatus::Completed;

    // Commit transaction — all field inserts + step updates are atomic
    tx.commit().await.internal_err()?;

    // Only email on meaningful cross-role transitions (user→admin review needed)
    if step_status == process::state_machine::StepStatus::PendingReview {
        let step_name = find_step_name(&template, payload.template_step_id);
        if let Some(ctx) = build_notification_ctx(
            &state.pg_pool,
            info.user_id,
            &state.super_admin_email,
            &template.name,
        )
        .await
        {
            process::notification::notify_admin_pending_review(
                &ctx,
                payload.process_id,
                &step_name,
            )
            .await;
        }
    }

    Ok(Json(SaveStepResponse {
        status: "success".to_string(),
        message: "Step saved successfully".to_string(),
        step: SavedStepInfo {
            template_step_id: payload.template_step_id,
            is_completed,
        },
    }))
}

/// POST /api/v1/admin/process/review
pub async fn admin_review_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminReviewRequest>,
) -> Result<Json<AdminReviewResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.internal_err()?;

    let (info, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, true).await?;

    let flagged = payload.flagged_step_ids.as_deref().unwrap_or(&[]);
    let is_completed = process::db::review_step(
        &mut tx,
        payload.process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: payload.template_step_id,
            reviewer_user_id: Some(user.user_id),
            is_approved: payload.is_approved,
            notes: payload.notes.as_deref(),
            flagged_step_ids: flagged,
        },
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let message = if payload.is_approved {
        "Review recorded successfully. Step is marked as completed.".to_string()
    } else {
        "Review recorded successfully. Step is marked as incomplete.".to_string()
    };

    tx.commit().await.internal_err()?;

    let step_name = find_step_name(&template, payload.template_step_id);
    if let Some(ctx) = build_notification_ctx(
        &state.pg_pool,
        info.user_id,
        &state.super_admin_email,
        &template.name,
    )
    .await
    {
        // Resolve flagged step IDs to human-readable names for the email
        let flagged_names: Vec<String> = payload
            .flagged_step_ids
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|&id| find_step_name(&template, id))
            .collect();

        process::notification::notify_user_review(
            &ctx,
            process::notification::ReviewNotificationParams {
                process_id: payload.process_id,
                step_name: &step_name,
                is_approved: payload.is_approved,
                notes: payload.notes.as_deref(),
                flagged_step_names: &flagged_names,
            },
        )
        .await;
    }

    Ok(Json(AdminReviewResponse {
        status: "success".to_string(),
        message,
        is_completed,
    }))
}

/// POST /api/v1/admin/process/revoke
pub async fn admin_revoke_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminRevokeRequest>,
) -> Result<Json<AdminRevokeResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.internal_err()?;

    let (info, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, true).await?;

    process::db::reopen_step(
        &mut tx,
        payload.process_id,
        &template,
        payload.template_step_id,
        user.user_id,
        payload.notes.as_deref(),
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;
    tx.commit().await.internal_err()?;

    let step_name = find_step_name(&template, payload.template_step_id);
    if let Some(ctx) = build_notification_ctx(
        &state.pg_pool,
        info.user_id,
        &state.super_admin_email,
        &template.name,
    )
    .await
    {
        process::notification::notify_user_revoke(
            &ctx,
            payload.process_id,
            &step_name,
            payload.notes.as_deref(),
        )
        .await;
    }

    Ok(Json(AdminRevokeResponse {
        status: "success".to_string(),
        message: "Step revoked successfully".to_string(),
    }))
}

/// POST /api/v1/admin/process/add-note
pub async fn admin_add_note(
    user: SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminAddNoteRequest>,
) -> Result<Json<AdminAddNoteResponse>, ApiError> {
    let comment = payload.comment.trim();
    if comment.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Comment cannot be empty".to_string()),
        ));
    }

    let mut conn = state.pg_pool.acquire().await.internal_err()?;

    // Verify the process exists and admin has access
    load_authorized_process(&mut conn, &user.0, payload.process_id, true).await?;

    process::db::insert_admin_comment(
        &mut conn,
        payload.process_id,
        payload.template_step_id,
        user.0.user_id,
        comment,
    )
    .await
    .internal_err()?;

    Ok(Json(AdminAddNoteResponse {
        status: "success".to_string(),
        message: "Internal note added".to_string(),
    }))
}
