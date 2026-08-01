use crate::document::SignatureRequirement;
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use hmac::Mac;
use uuid::Uuid;

type ApiError = (StatusCode, Json<String>);

fn internal_err(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string()))
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}

fn forbidden(msg: impl Into<String>) -> ApiError {
    (StatusCode::FORBIDDEN, Json(msg.into()))
}

/// Only the signer a requirement is actually waiting on may sign it — signing is a
/// legal act tied to a specific person, so nobody (not even a reviewer) can complete
/// it on someone else's behalf.
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

/// A field can declare which designations are eligible to sign it
/// (`required_signs_from_designations`). Re-checked at the moment of signing (not just when
/// the reviewer attaches the signer) so a designation revoked in between makes the
/// signature invalid — the signer must currently hold one of the listed designations.
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

/// Recompute a step's completion after a signature requirement's status changed,
/// WITHOUT repointing the field at any signed document — used when a completed sign
/// still leaves other attached signers outstanding. Also notifies whichever manual
/// signer just became actionable (SignYu manages its own signer notifications).
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

/// Repoints the field at the newly signed document (only valid once every attached
/// signer has signed) and recomputes gating.
async fn attach_replacement_and_recompute_gating(
    state: &AppState,
    requirement: &SignatureRequirement,
    new_document_id: Uuid,
) -> Result<(), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, requirement.process_id).await?;

    // The original document may have already been superseded by a later save (this
    // requirement is stale) — only repoint the field if it's still pointing at it.
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

    // Only notify the resident if THIS signature completion is what pushed the step
    // over the line into Completed — never for a mid-chain signature, and never if the
    // step needed something else besides this signature and is still incomplete.
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
    /// When given, scopes to that one document's requirements (one per attached
    /// signer) instead of every requirement in the system.
    #[serde(default)]
    pub document_id: Option<Uuid>,
}

/// POST /api/v1/documents/signatures/all
/// With no `document_id`: every requirement regardless of status — lets the admin
/// document list build a document_id <-> signed_document_id lineage map in one call
/// instead of a lookup per row. With `document_id`: just that document's requirements.
/// The body itself is optional (existing callers send no body at all) — anything that
/// isn't valid JSON is just treated as "no filter".
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
    /// Whether the server has `SIGNYU_API_KEY` configured — the actual on/off switch
    /// for offering "signyu" as a provider choice when attaching signers (see
    /// `main.rs`'s `signyu_client` construction). Lets the Add Signers UI hide the
    /// SignYu option entirely rather than offering a choice that will 400.
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
/// Lets the signer a requirement is actually waiting on view the document before they
/// sign it — the original for the first signer in the chain, otherwise the PREVIOUS
/// signer's signed copy, so they're signing on top of what came before them, not a
/// blind copy of the untouched original. Same authorization as the sign endpoints:
/// only that requirement's own signer, and only while it's still PENDING.
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
        .signature_replace_confirm_upload(
            payload.requirement_id,
            payload.new_document_id,
            user.user_id,
        )
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

/// Polls the external provider for `external_document_id` and, if it's now COMPLETED,
/// ingests the signed PDF and unblocks the process field. Shared by the manual
/// "Refresh Status" button and the webhook handler below — the webhook doesn't trust
/// its own payload fields (the exact SignYu webhook contract wasn't available when this
/// was written), it just uses receipt of ANY webhook hit as a signal to re-poll here.
async fn reconcile_external_document(
    state: &AppState,
    external_document_id: &str,
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

    state
        .document_manager
        .signature_ingest_provider_completed_document(external_document_id, pdf_bytes)
        .await
        .map_err(internal_err)?;

    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let template = load_template(&mut conn, process_id).await?;

    let still_current =
        process::db::is_current_field_value(&mut conn, process_id, template_field_id, document_id)
            .await
            .map_err(internal_err)?;

    if still_current {
        // The new signed document was created inside `signature_ingest_provider_completed_document`
        // — look it up via the just-updated requirements so we repoint the field at it.
        if let Ok(updated) = state
            .document_manager
            .signature_requirements_for_external_document(external_document_id)
            .await
        {
            if let Some(signed_document_id) = updated.iter().find_map(|r| r.signed_document_id) {
                process::db::attach_signed_replacement(
                    &mut conn,
                    process_id,
                    template_step_id,
                    template_field_id,
                    signed_document_id,
                )
                .await
                .map_err(internal_err)?;
            }
        }
    }

    let statuses_before = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .map_err(internal_err)?;
    let was_completed = statuses_before.get(&template_step_id)
        == Some(&process::state_machine::StepStatus::Completed);

    process::db::recompute_gating_for_step(&mut conn, &template, process_id, template_step_id)
        .await
        .map_err(internal_err)?;

    if !was_completed {
        let statuses_after = process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .map_err(internal_err)?;
        if statuses_after.get(&template_step_id)
            == Some(&process::state_machine::StepStatus::Completed)
        {
            if let Ok(Some(info)) = process::db::get_process_info(&mut conn, process_id).await {
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

/// SignYu's two webhook events (`signer.signed`, `document.completed`) — see the
/// Webhooks doc. We don't currently branch on which one fired (both just trigger a
/// re-poll of `get_document`, see `signyu_webhook` below), but the field is parsed and
/// kept so it lands in logs/error messages if something about a payload looks wrong.
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

/// Verifies a SignYu webhook's `X-SignSetu-Signature` header — `sha256=<hex>`, an
/// HMAC-SHA256 of the exact raw request body bytes, keyed with the endpoint's signing
/// secret (SignYu dashboard → Developers). Per SignYu's docs this MUST be computed over
/// the raw bytes, not a re-serialized object, which is exactly why this handler takes
/// `Bytes` instead of a `Json<T>` extractor — deserializing first and re-serializing
/// later would not reproduce the same bytes SignYu signed. `hmac::Mac::verify_slice`
/// does the comparison in constant time internally, per SignYu's "always compare using
/// a constant time function" requirement.
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

/// POST /webhooks/signyu — public (no session). SignYu fires this on `signer.signed`
/// and `document.completed`. Requires `SIGNYU_WEBHOOK_SECRET` to be configured — an
/// unverifiable webhook (no secret configured, missing/malformed header, or a
/// signature that doesn't match) is rejected outright rather than trusted.
///
/// Once verified, we still don't trust the payload's own `status`/`signer`/`signers`
/// fields as authoritative for mutating state — we only use `documentId` as a signal
/// to re-poll `get_document` (the exact same path the manual "Refresh Status" button
/// uses), so there is only ever one code path that actually writes signature
/// completion state, regardless of how it was triggered.
///
/// Idempotent by construction: SignYu retries on a non-2xx response or a 10s timeout,
/// which means the same event can be redelivered — `reconcile_external_document` is a
/// no-op once nothing is left PENDING for the document, so a redelivered event can
/// never double-apply a completion.
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
