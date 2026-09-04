//! Write-path process transitions: saving a step's fields, admin review/revoke,
//! and standalone admin notes.

use super::{
    build_actor_context, build_notification_ctx, find_step_name, load_authorized_process,
    notify_newly_opened_designation_steps, ApiError, ResultExt,
};
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

    let actor = build_actor_context(&state.permissions_manager, &user).await;
    // process.view is read-only oversight; never grants write access here.
    let (info, template) = load_authorized_process(
        &mut tx,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;

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
                // Process fields only ever hold flat-scoped (RESIDENT_SPECIFIC) documents.
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
        &actor,
        user.user_id,
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
    notify_newly_opened_designation_steps(
        &state.permissions_manager,
        &template,
        payload.process_id,
        &save_outcome.newly_opened_step_ids,
    )
    .await;

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

    let actor = build_actor_context(&state.permissions_manager, &user).await;
    let (info, template) = load_authorized_process(
        &mut tx,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;

    let flagged = payload.flagged_step_ids.as_deref().unwrap_or(&[]);
    let review_outcome = process::db::review_step(
        &mut tx,
        payload.process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: payload.template_step_id,
            reviewer_user_id: Some(user.user_id),
            is_approved: payload.is_approved,
            notes: payload.notes.as_deref(),
            flagged_step_ids: flagged,
            acted_as_designation: None,
        },
        &actor,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;
    let is_completed = review_outcome.is_completed;

    let message = if payload.is_approved {
        "Review recorded successfully. Step is marked as completed.".to_string()
    } else {
        "Review recorded successfully. Step is marked as incomplete.".to_string()
    };

    tx.commit().await.internal_err()?;
    notify_newly_opened_designation_steps(
        &state.permissions_manager,
        &template,
        payload.process_id,
        &review_outcome.newly_opened_step_ids,
    )
    .await;

    let step_name = find_step_name(&template, payload.template_step_id);
    let wants_notification = state
        .permissions_manager
        .get_notification_prefs(info.user_id)
        .await
        .map(|p| p.notify_process_updates)
        .unwrap_or(true);
    if wants_notification {
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

    let actor = build_actor_context(&state.permissions_manager, &user).await;
    let (info, template) = load_authorized_process(
        &mut tx,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;

    let newly_opened_step_ids = process::db::reopen_step(
        &mut tx,
        payload.process_id,
        &template,
        payload.template_step_id,
        user.user_id,
        payload.notes.as_deref(),
        &actor,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;
    tx.commit().await.internal_err()?;
    notify_newly_opened_designation_steps(
        &state.permissions_manager,
        &template,
        payload.process_id,
        &newly_opened_step_ids,
    )
    .await;

    let step_name = find_step_name(&template, payload.template_step_id);
    let wants_notification = state
        .permissions_manager
        .get_notification_prefs(info.user_id)
        .await
        .map(|p| p.notify_process_updates)
        .unwrap_or(true);
    if wants_notification {
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

    // Verify the process exists and the caller has access to it
    let actor = build_actor_context(&state.permissions_manager, &user.0).await;
    let (_info, template) = load_authorized_process(
        &mut conn,
        &user.0,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;

    let step_owner = process::state_machine::build_step_info_map(&template)
        .get(&payload.template_step_id)
        .map(|info| info.filled_by.clone())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json("Step not found".to_string())))?;
    if !actor.may_review(&step_owner) {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: you do not act on this step".to_string()),
        ));
    }

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

#[derive(Deserialize)]
pub struct AddDocumentSignersRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub template_field_id: i32,
    pub document_id: Uuid,
    pub signer_ids: Vec<i32>,
    /// "manual" (default) or a registered external provider name ("signyu").
    #[serde(default = "default_provider")]
    pub provider: String,
}

fn default_provider() -> String {
    "manual".to_string()
}

#[derive(Serialize)]
pub struct AddDocumentSignersResponse {
    pub status: String,
    pub requirements: Vec<crate::document::SignatureRequirement>,
}

/// POST /api/v1/admin/process/documents/add-signers
/// Attaches signers to a document field in order — the field stays blocked until
/// every attached signer has signed.
pub async fn admin_add_document_signers(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AddDocumentSignersRequest>,
) -> Result<Json<AddDocumentSignersResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    let actor = build_actor_context(&state.permissions_manager, &user).await;
    let (_info, template) = load_authorized_process(
        &mut conn,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;

    let statuses = process::db::get_statuses(&mut conn, payload.process_id, &template)
        .await
        .internal_err()?;
    if statuses.get(&payload.template_step_id) != Some(&process::state_machine::StepStatus::Open) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Signers can only be attached while the step is still open".to_string()),
        ));
    }

    let step_owner = process::state_machine::build_step_info_map(&template)
        .get(&payload.template_step_id)
        .map(|info| info.filled_by.clone())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json("Step not found".to_string())))?;
    if !actor.may_review(&step_owner) {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: you do not act on this step".to_string()),
        ));
    }

    // template_field_id, document_id, and template_step_id are all client-supplied —
    // verify they actually belong together, not an arbitrary combination the caller
    // happens to have access to (which for provider="signyu" would send the wrong PDF
    // to real signers via the live e-sign integration).
    let field_belongs_to_step = process::state_machine::build_step_info_map(&template)
        .get(&payload.template_step_id)
        .map(|info| {
            info.fields
                .iter()
                .any(|f| f.field_id == payload.template_field_id)
        })
        .unwrap_or(false);
    if !field_belongs_to_step {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("template_field_id does not belong to template_step_id".to_string()),
        ));
    }
    let is_current_document = process::db::is_current_field_value(
        &mut conn,
        payload.process_id,
        payload.template_field_id,
        payload.document_id,
    )
    .await
    .internal_err()?;
    if !is_current_document {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("document_id is not the current document for this field".to_string()),
        ));
    }

    if payload.provider == "signyu" {
        if state.signyu_client.is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json("SignYu is not configured on this server".to_string()),
            ));
        }
        let content_type: Option<String> =
            sqlx::query_scalar("SELECT content_type FROM documents WHERE document_id = $1")
                .bind(payload.document_id)
                .fetch_optional(&mut *conn)
                .await
                .internal_err()?
                .flatten();
        if content_type.as_deref() != Some("application/pdf") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json("SignYu only supports PDF documents".to_string()),
            ));
        }
    }

    validate_signer_order_against_designations(&state, &template, &payload).await?;

    let mut requirements = state
        .document_manager
        .signature_add_signers_to_document(
            payload.document_id,
            payload.process_id,
            payload.template_step_id,
            payload.template_field_id,
            &payload.signer_ids,
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    if !requirements.is_empty() && payload.provider == "signyu" {
        requirements = send_new_requirements_via_signyu(&state, requirements).await?;
    }

    process::db::recompute_gating_for_step(
        &mut conn,
        &template,
        payload.process_id,
        payload.template_step_id,
    )
    .await
    .internal_err()?;

    // SignYu emails its own signing links directly — only send our own notification
    // for the manual flow, so a signer never gets two different "please sign" emails.
    if !requirements.is_empty() && payload.provider != "signyu" {
        let step_name = find_step_name(&template, payload.template_step_id);
        let document_title = state
            .document_manager
            .admin_get_any_document(payload.document_id)
            .await
            .ok()
            .flatten()
            .map(|d| d.document.title)
            .unwrap_or_else(|| "Document".to_string());

        for req in &requirements {
            let wants_notification = state
                .permissions_manager
                .get_notification_prefs(req.signer_user_id)
                .await
                .map(|p| p.notify_signature_events)
                .unwrap_or(true);
            if wants_notification {
                process::notification::notify_signature_queue(
                    &req.signer_email,
                    &template.name,
                    &step_name,
                    &document_title,
                    payload.process_id,
                )
                .await;
            }
        }
    }

    Ok(Json(AddDocumentSignersResponse {
        status: "success".to_string(),
        requirements,
    }))
}

/// If the field declares `required_signs_from_designations`, every signer must hold one
/// of the listed designations, in non-decreasing order of designation rank. Empty list
/// = no constraint.
async fn validate_signer_order_against_designations(
    state: &AppState,
    template: &process::ProcessTemplate,
    payload: &AddDocumentSignersRequest,
) -> Result<(), ApiError> {
    let step_info_map = process::state_machine::build_step_info_map(template);
    let required = step_info_map
        .get(&payload.template_step_id)
        .and_then(|info| {
            info.fields
                .iter()
                .find(|f| f.field_id == payload.template_field_id)
        })
        .map(|f| f.required_signs_from_designations.clone())
        .unwrap_or_default();

    if required.is_empty() || payload.signer_ids.is_empty() {
        return Ok(());
    }

    let designation_map = state
        .document_manager
        .designation_codes_for_signers(&payload.signer_ids)
        .await
        .internal_err()?;

    let mut last_rank: Option<usize> = None;
    for &signer_id in &payload.signer_ids {
        let codes = designation_map.get(&signer_id).cloned().unwrap_or_default();
        let rank = required.iter().position(|d| codes.contains(d));
        let rank = rank.ok_or_else(|| {
            bad_request_err(format!(
                "Signer {} does not hold any of the designations this field requires ({}).",
                signer_id,
                required.join(", ")
            ))
        })?;
        if let Some(prev) = last_rank {
            if rank < prev {
                return Err(bad_request_err(format!(
                    "Signers must be added in the order required by this field's designations ({}).",
                    required.join(", ")
                )));
            }
        }
        last_rank = Some(rank);
    }

    Ok(())
}

/// Sends the document via SignYu and records the returned external ids on each requirement.
async fn send_new_requirements_via_signyu(
    state: &AppState,
    requirements: Vec<crate::document::SignatureRequirement>,
) -> Result<Vec<crate::document::SignatureRequirement>, ApiError> {
    let signyu = state
        .signyu_client
        .as_ref()
        .ok_or_else(|| bad_request_err("SignYu is not configured on this server"))?;

    let document_id = requirements[0].document_id;
    let original = state
        .document_manager
        .admin_get_any_document(document_id)
        .await
        .internal_err()?
        .ok_or_else(|| bad_request_err("Original document not found"))?
        .document;

    let key = crate::document::derive_s3_key(document_id);
    let pdf_bytes = state
        .document_manager
        .s3_accessor
        .download_file(&key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?
        .collect()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?
        .into_bytes()
        .to_vec();

    let signer_contacts: Vec<crate::signature_providers::SignerContact> = requirements
        .iter()
        .map(|r| crate::signature_providers::SignerContact {
            name: r.signer_name.clone(),
            phone: r.signer_phone.clone(),
            email: r.signer_email.clone(),
        })
        .collect();

    let result = signyu
        .create_and_send(
            pdf_bytes,
            &original.title,
            &signer_contacts,
            Some(requirements[0].requirement_id),
        )
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(e)))?;

    let requirement_ids: Vec<i32> = requirements.iter().map(|r| r.requirement_id).collect();
    state
        .document_manager
        .signature_set_external_tracking_batch(
            &requirement_ids,
            "signyu",
            &result.external_document_id,
            &result.external_signer_ids,
            &result.sign_urls,
        )
        .await
        .internal_err()?;

    state
        .document_manager
        .signature_get_requirements_batch(&requirement_ids)
        .await
        .internal_err()
}

fn bad_request_err(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}
