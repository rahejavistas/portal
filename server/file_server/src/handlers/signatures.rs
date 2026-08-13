use crate::document::SignatureRequirement;
use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, forbidden, internal_err, ApiError};
use crate::state::AppState;
use axum::{extract::State, Json};
use hmac::Mac;
use uuid::Uuid;

/// Only the requirement's own signer may act on it — not even a reviewer can sign on
/// someone else's behalf.
fn can_act_on_requirement(user: &authentication::User, requirement: &SignatureRequirement) -> bool {
    user.user_id == requirement.signer_user_id
}

async fn load_template(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<process::ProcessTemplate, ApiError> {
    let info = process::db::get_process_info(&mut *conn, process_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| internal_err("Process not found"))?;
    process::template_store::load_template_version(&mut *conn, info.template_version_id)
        .await
        .map_err(internal_err)
}

fn field_required_designations(
    template: &process::ProcessTemplate,
    step_id: i32,
    field_id: i32,
) -> Vec<String> {
    process::state_machine::build_step_info_map(template)
        .get(&step_id)
        .and_then(|info| info.fields.iter().find(|f| f.field_id == field_id))
        .map(|f| f.required_signs_from_designations.clone())
        .unwrap_or_default()
}

/// Re-checks the signer still holds a required designation at signing time, not just
/// at attach time.
async fn ensure_signer_still_eligible(
    state: &AppState,
    requirement: &SignatureRequirement,
) -> Result<(), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, requirement.process_id).await?;
    let required = field_required_designations(
        &template,
        requirement.template_step_id,
        requirement.template_field_id,
    );
    if required.is_empty() {
        return Ok(());
    }

    let designation_map = state
        .document_manager
        .designation_codes_for_signers(&[requirement.signer_id])
        .await
        .map_err(internal_err)?;
    let held = designation_map
        .get(&requirement.signer_id)
        .cloned()
        .unwrap_or_default();

    if !required.iter().any(|d| held.contains(d)) {
        return Err(forbidden(format!(
            "You no longer hold a designation required to sign this document ({}).",
            required.join(", ")
        )));
    }
    Ok(())
}

/// Recomputes step completion without repointing the field — used when other signers
/// are still outstanding. Notifies the next manual signer, if any.
async fn recompute_gating_only(
    state: &AppState,
    requirement: &SignatureRequirement,
) -> Result<(), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, requirement.process_id).await?;
    process::db::recompute_gating_for_step(
        &mut conn,
        &template,
        requirement.process_id,
        requirement.template_step_id,
    )
    .await
    .map_err(internal_err)?;

    if let Ok(siblings) = state
        .document_manager
        .signature_requirements_for_document(requirement.document_id)
        .await
    {
        if let Some(next) = siblings
            .iter()
            .filter(|r| {
                r.status == SignatureStatus::Pending && r.sign_order > requirement.sign_order
            })
            .min_by_key(|r| r.sign_order)
        {
            let wants_notification = state
                .permissions_manager
                .get_notification_prefs(next.signer_user_id)
                .await
                .map(|p| p.notify_signature_events)
                .unwrap_or(true);
            if next.provider == "manual" && wants_notification {
                let step_name = crate::handlers::process::find_step_name(
                    &template,
                    requirement.template_step_id,
                );
                process::notification::notify_signature_turn(
                    &next.signer_email,
                    &template.name,
                    &step_name,
                    &next.document_title,
                    requirement.process_id,
                )
                .await;
            }
        }
    }

    Ok(())
}

/// Repoints the field at the signed document (once every signer has signed) and
/// recomputes gating.
async fn attach_replacement_and_recompute_gating(
    state: &AppState,
    requirement: &SignatureRequirement,
    new_document_id: Uuid,
) -> Result<(), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, requirement.process_id).await?;

    // Skip if the field was already superseded by a later save.
    let still_current = process::db::is_current_field_value(
        &mut conn,
        requirement.process_id,
        requirement.template_field_id,
        requirement.document_id,
    )
    .await
    .map_err(internal_err)?;

    if still_current {
        process::db::attach_signed_replacement(
            &mut conn,
            requirement.process_id,
            requirement.template_step_id,
            requirement.template_field_id,
            new_document_id,
        )
        .await
        .map_err(internal_err)?;
    }

    let statuses_before = process::db::get_statuses(&mut conn, requirement.process_id, &template)
        .await
        .map_err(internal_err)?;
    let was_completed = statuses_before.get(&requirement.template_step_id)
        == Some(&process::state_machine::StepStatus::Completed);

    process::db::recompute_gating_for_step(
        &mut conn,
        &template,
        requirement.process_id,
        requirement.template_step_id,
    )
    .await
    .map_err(internal_err)?;

    // Only notify if this signature is what completed the step, not a mid-chain one.
    if !was_completed {
        let statuses_after =
            process::db::get_statuses(&mut conn, requirement.process_id, &template)
                .await
                .map_err(internal_err)?;
        if statuses_after.get(&requirement.template_step_id)
            == Some(&process::state_machine::StepStatus::Completed)
        {
            if let Ok(Some(info)) =
                process::db::get_process_info(&mut conn, requirement.process_id).await
            {
                let wants_notification = state
                    .permissions_manager
                    .get_notification_prefs(info.user_id)
                    .await
                    .map(|p| p.notify_signature_events)
                    .unwrap_or(true);
                if wants_notification {
                    if let Some(ctx) = crate::handlers::process::build_notification_ctx(
                        &state.pg_pool,
                        info.user_id,
                        &state.super_admin_email,
                        &template.name,
                    )
                    .await
                    {
                        let step_name = crate::handlers::process::find_step_name(
                            &template,
                            requirement.template_step_id,
                        );
                        process::notification::notify_signature_step_complete(
                            &ctx,
                            requirement.process_id,
                            &step_name,
                        )
                        .await;
                    }
                }
            }
        }
    }

    Ok(())
}

/// POST /api/v1/documents/signatures/queue
/// Requirements waiting on the caller specifically, and only once it's their turn.
pub async fn queue(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Json<Vec<SignatureRequirement>>, ApiError> {
    let items = state
        .document_manager
        .signature_queue_list_for_user(user.user_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(items))
}

#[derive(serde::Deserialize, Default)]
pub struct AllPayload {
    /// When given, scopes to just that document's requirements.
    #[serde(default)]
    pub document_id: Option<Uuid>,
}

/// POST /api/v1/documents/signatures/all
/// No document_id: every requirement in the system. Body is optional — invalid/missing
/// JSON is treated as "no filter".
pub async fn all(
    State(state): State<AppState>,
    _session_user: SessionUser,
    body: axum::body::Bytes,
) -> Result<Json<Vec<SignatureRequirement>>, ApiError> {
    let payload: AllPayload = serde_json::from_slice(&body).unwrap_or_default();
    let items = match payload.document_id {
        Some(document_id) => {
            state
                .document_manager
                .signature_requirements_for_document(document_id)
                .await
        }
        None => state.document_manager.signature_list_all().await,
    }
    .map_err(internal_err)?;
    Ok(Json(items))
}

#[derive(serde::Serialize)]
pub struct ProviderStatusResponse {
    /// Whether SIGNYU_API_KEY is configured — lets the UI hide the SignYu option.
    pub signyu_available: bool,
}

/// POST /api/v1/admin/documents/signatures/provider-status
pub async fn provider_status(
    State(state): State<AppState>,
    _session_user: SessionUser,
) -> Json<ProviderStatusResponse> {
    Json(ProviderStatusResponse {
        signyu_available: state.signyu_client.is_some(),
    })
}

#[derive(serde::Deserialize)]
pub struct DocumentDownloadUrlPayload {
    pub requirement_id: i32,
}

/// POST /api/v1/admin/documents/signatures/document-download-url
/// Lets the signer preview the document to sign — the original, or the previous
/// signer's copy if mid-chain.
pub async fn document_download_url(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<DocumentDownloadUrlPayload>,
) -> Result<Json<crate::document::DocumentWithMetadata>, ApiError> {
    let requirement = state
        .document_manager
        .signature_get_requirement(payload.requirement_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| bad_request("Signature requirement not found"))?;

    if !can_act_on_requirement(&user, &requirement) {
        return Err(forbidden(
            "You are not the signer this requirement is waiting on",
        ));
    }

    let input_document_id = state
        .document_manager
        .input_document_id_for_requirement(&requirement)
        .await
        .map_err(internal_err)?;

    let doc = state
        .document_manager
        .admin_get_any_document(input_document_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| bad_request("Document not found"))?;

    Ok(Json(doc))
}

#[derive(serde::Deserialize)]
pub struct ReplaceUploadUrlPayload {
    pub requirement_id: i32,
    pub content_type: String,
}

#[derive(serde::Serialize)]
pub struct ReplaceUploadUrlResponse {
    pub new_document_id: Uuid,
    pub upload_url: String,
}

/// POST /api/v1/admin/documents/signatures/replace-upload-url
pub async fn replace_upload_url(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<ReplaceUploadUrlPayload>,
) -> Result<Json<ReplaceUploadUrlResponse>, ApiError> {
    let requirement = state
        .document_manager
        .signature_get_requirement(payload.requirement_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| bad_request("Signature requirement not found"))?;

    if !can_act_on_requirement(&user, &requirement) {
        return Err(forbidden(
            "You are not the signer this requirement is waiting on",
        ));
    }

    ensure_signer_still_eligible(&state, &requirement).await?;

    let (new_document_id, upload_url) = state
        .document_manager
        .signature_replace_upload_url(payload.requirement_id, &payload.content_type, user.user_id)
        .await
        .map_err(bad_request)?;

    Ok(Json(ReplaceUploadUrlResponse {
        new_document_id,
        upload_url,
    }))
}

#[derive(serde::Deserialize)]
pub struct ReplaceConfirmUploadPayload {
    pub requirement_id: i32,
    pub new_document_id: Uuid,
}

/// POST /api/v1/admin/documents/signatures/replace-confirm-upload
pub async fn replace_confirm_upload(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<ReplaceConfirmUploadPayload>,
) -> Result<Json<SignatureRequirement>, ApiError> {
    let existing = state
        .document_manager
        .signature_get_requirement(payload.requirement_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| bad_request("Signature requirement not found"))?;

    if !can_act_on_requirement(&user, &existing) {
        return Err(forbidden(
            "You are not the signer this requirement is waiting on",
        ));
    }

    ensure_signer_still_eligible(&state, &existing).await?;

    let (requirement, all_signed) = state
        .document_manager
        .signature_replace_confirm_upload(&existing, payload.new_document_id, user.user_id)
        .await
        .map_err(bad_request)?;

    if all_signed {
        attach_replacement_and_recompute_gating(&state, &requirement, payload.new_document_id)
            .await?;
    } else {
        recompute_gating_only(&state, &requirement).await?;
    }

    Ok(Json(requirement))
}

/// Polls the provider for `external_document_id`; if COMPLETED, ingests the signed PDF
/// and unblocks the field. Shared by "Refresh Status" and the webhook handler.
///
/// SignYu redelivers webhooks and the manual refresh can race a webhook, so the whole
/// body runs under a session-level advisory lock keyed on `external_document_id` — a
/// concurrent call just no-ops instead of double-ingesting the signed PDF. Everything
/// runs on the single connection holding the lock (never a second pool connection at
/// the same time), so this can't itself exhaust the pool; if the unlock can't be
/// confirmed to have run, the connection is closed instead of returned to the pool,
/// so a held lock can never get silently recycled into a future request.
async fn reconcile_external_document(
    state: &AppState,
    external_document_id: &str,
) -> Result<(), ApiError> {
    let mut lock_conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let (got_lock,): (bool,) = sqlx::query_as("SELECT pg_try_advisory_lock(hashtext($1)::bigint)")
        .bind(external_document_id)
        .fetch_one(&mut *lock_conn)
        .await
        .map_err(internal_err)?;
    if !got_lock {
        return Ok(());
    }

    let result =
        reconcile_external_document_locked(state, external_document_id, &mut lock_conn).await;

    let unlocked: Result<(bool,), _> =
        sqlx::query_as("SELECT pg_advisory_unlock(hashtext($1)::bigint)")
            .bind(external_document_id)
            .fetch_one(&mut *lock_conn)
            .await;
    match unlocked {
        Ok((true,)) => {}
        _ => lock_conn.close().await.unwrap_or(()),
    }

    result
}

async fn reconcile_external_document_locked(
    state: &AppState,
    external_document_id: &str,
    conn: &mut sqlx::PgConnection,
) -> Result<(), ApiError> {
    let signyu = state
        .signyu_client
        .as_ref()
        .ok_or_else(|| internal_err("SignYu is not configured on this server"))?;

    let status = signyu
        .get_document(external_document_id, None)
        .await
        .map_err(internal_err)?;

    if !status.is_completed {
        return Ok(());
    }
    let Some(download_url) = status.download_url else {
        return Ok(());
    };

    let requirements_before = state
        .document_manager
        .signature_requirements_for_external_document(external_document_id)
        .await
        .map_err(internal_err)?;
    let Some(sample) = requirements_before.first() else {
        return Ok(());
    };
    if requirements_before
        .iter()
        .all(|r| r.status != SignatureStatus::Pending)
    {
        return Ok(()); // Already reconciled.
    }

    let pdf_bytes = signyu
        .download_signed_pdf(&download_url)
        .await
        .map_err(internal_err)?;

    let process_id = sample.process_id;
    let template_step_id = sample.template_step_id;
    let document_id = sample.document_id;
    let template_field_id = sample.template_field_id;

    let updated = state
        .document_manager
        .signature_ingest_provider_completed_document(external_document_id, pdf_bytes)
        .await
        .map_err(internal_err)?;

    let template = load_template(conn, process_id).await?;

    let still_current =
        process::db::is_current_field_value(conn, process_id, template_field_id, document_id)
            .await
            .map_err(internal_err)?;

    if still_current {
        if let Some(signed_document_id) = updated.iter().find_map(|r| r.signed_document_id) {
            process::db::attach_signed_replacement(
                conn,
                process_id,
                template_step_id,
                template_field_id,
                signed_document_id,
            )
            .await
            .map_err(internal_err)?;
        }
    }

    let statuses_before = process::db::get_statuses(conn, process_id, &template)
        .await
        .map_err(internal_err)?;
    let was_completed = statuses_before.get(&template_step_id)
        == Some(&process::state_machine::StepStatus::Completed);

    process::db::recompute_gating_for_step(conn, &template, process_id, template_step_id)
        .await
        .map_err(internal_err)?;

    if !was_completed {
        let statuses_after = process::db::get_statuses(conn, process_id, &template)
            .await
            .map_err(internal_err)?;
        if statuses_after.get(&template_step_id)
            == Some(&process::state_machine::StepStatus::Completed)
        {
            if let Ok(Some(info)) = process::db::get_process_info(conn, process_id).await {
                if let Some(ctx) = crate::handlers::process::build_notification_ctx(
                    &state.pg_pool,
                    info.user_id,
                    &state.super_admin_email,
                    &template.name,
                )
                .await
                {
                    let step_name =
                        crate::handlers::process::find_step_name(&template, template_step_id);
                    process::notification::notify_signature_step_complete(
                        &ctx, process_id, &step_name,
                    )
                    .await;
                }
            }
        }
    }

    Ok(())
}

use crate::document::SignatureStatus;

#[derive(serde::Deserialize)]
pub struct RefreshStatusPayload {
    pub document_id: Uuid,
}

/// POST /api/v1/admin/documents/signatures/refresh-status
/// Manual fallback for "Refresh Status" in the UI — polls the provider instead of
/// waiting for its webhook. A no-op for `manual`-provider documents.
pub async fn refresh_status(
    State(state): State<AppState>,
    _session_user: SessionUser,
    Json(payload): Json<RefreshStatusPayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let requirements = state
        .document_manager
        .signature_requirements_for_document(payload.document_id)
        .await
        .map_err(internal_err)?;

    let Some(external_document_id) = requirements
        .iter()
        .find_map(|r| r.external_document_id.clone())
    else {
        return Ok(Json(
            serde_json::json!({ "status": "ok", "reconciled": false }),
        ));
    };

    reconcile_external_document(&state, &external_document_id).await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "reconciled": true }),
    ))
}

/// SignYu's two webhook events (signer.signed, document.completed) — both just
/// trigger a re-poll, so we don't branch on which one fired.
#[derive(serde::Deserialize)]
pub struct SignYuWebhookPayload {
    #[allow(dead_code)]
    event: Option<String>,
    #[serde(rename = "documentId")]
    document_id: Option<String>,
    #[serde(flatten)]
    _rest: serde_json::Value,
}

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// Verifies `X-SignSetu-Signature: sha256=<hex>`, an HMAC-SHA256 over the raw body
/// bytes — must be the raw bytes, not a re-serialized object, hence `Bytes` not `Json<T>`.
fn verify_signyu_signature(secret: &str, raw_body: &[u8], signature_header: &str) -> bool {
    let Some(hex_sig) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(signature_bytes) = hex::decode(hex_sig) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(raw_body);
    mac.verify_slice(&signature_bytes).is_ok()
}

/// POST /webhooks/signyu — public, no session. Rejects unverifiable webhooks (no
/// secret configured, bad header, or signature mismatch). Never trusts the payload's
/// own status fields — only uses `documentId` as a signal to re-poll `get_document`,
/// the same path "Refresh Status" uses. Idempotent: a redelivered event just re-polls
/// an already-reconciled document, which is a no-op.
pub async fn signyu_webhook(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(secret) = &state.signyu_webhook_secret else {
        return Err(internal_err(
            "SIGNYU_WEBHOOK_SECRET is not configured — refusing to process an unverifiable webhook",
        ));
    };

    let signature_header = headers
        .get("X-SignSetu-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| forbidden("Missing X-SignSetu-Signature header"))?;

    if !verify_signyu_signature(secret, &body, signature_header) {
        return Err(forbidden("Invalid webhook signature"));
    }

    let payload: SignYuWebhookPayload = serde_json::from_slice(&body)
        .map_err(|e| bad_request(format!("Invalid webhook payload: {}", e)))?;

    let Some(document_id) = payload.document_id else {
        return Ok(Json(serde_json::json!({ "status": "ignored" })));
    };

    reconcile_external_document(&state, &document_id).await?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[cfg(test)]
mod signature_verification_tests {
    use super::*;

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn accepts_a_correctly_signed_body() {
        let body = br#"{"event":"document.completed","documentId":"abc"}"#;
        let header = sign("whsec_test", body);
        assert!(verify_signyu_signature("whsec_test", body, &header));
    }

    #[test]
    fn rejects_a_tampered_body() {
        let body = br#"{"event":"document.completed","documentId":"abc"}"#;
        let header = sign("whsec_test", body);
        let tampered_body = br#"{"event":"document.completed","documentId":"xyz"}"#;
        assert!(!verify_signyu_signature(
            "whsec_test",
            tampered_body,
            &header
        ));
    }

    #[test]
    fn rejects_the_wrong_secret() {
        let body = br#"{"event":"document.completed","documentId":"abc"}"#;
        let header = sign("whsec_test", body);
        assert!(!verify_signyu_signature("whsec_different", body, &header));
    }

    #[test]
    fn rejects_a_header_missing_the_sha256_prefix() {
        let body = br#"{"event":"document.completed","documentId":"abc"}"#;
        let mut mac = HmacSha256::new_from_slice(b"whsec_test").unwrap();
        mac.update(body);
        let raw_hex = hex::encode(mac.finalize().into_bytes());
        assert!(!verify_signyu_signature("whsec_test", body, &raw_hex));
    }

    #[test]
    fn rejects_non_hex_signature() {
        let body = br#"{"event":"document.completed","documentId":"abc"}"#;
        assert!(!verify_signyu_signature(
            "whsec_test",
            body,
            "sha256=not-hex-at-all"
        ));
    }

    #[test]
    fn rejects_empty_signature() {
        let body = br#"{"event":"document.completed","documentId":"abc"}"#;
        assert!(!verify_signyu_signature("whsec_test", body, "sha256="));
    }
}
