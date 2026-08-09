use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, forbidden, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::PaymentClaim;
use accounting::{ClaimId, InvoiceId};
use axum::{extract::State, Json};
use rust_decimal::Decimal;

use super::not_found;

#[derive(serde::Deserialize)]
pub struct SubmitClaimRequest {
    pub invoice_id: InvoiceId,
    pub amount: Decimal,
    pub payment_method: String,
    pub reference_number: String,
    pub note: Option<String>,
}

#[derive(serde::Serialize)]
pub struct SubmitClaimResponse {
    pub claim_id: ClaimId,
}

/// Resident-facing: creates a PENDING claim only, never posts to the ledger.
pub async fn submit_claim(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SubmitClaimRequest>,
) -> Result<(axum::http::StatusCode, Json<SubmitClaimResponse>), ApiError> {
    if payload.amount <= Decimal::ZERO {
        return Err(bad_request("amount must be positive"));
    }
    if payload.reference_number.trim().is_empty() {
        return Err(bad_request("reference_number is required"));
    }

    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let invoice = accounting::db::get_invoice(&mut conn, payload.invoice_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Invoice not found"))?;

    let owns_flat = accounting::db::user_owns_flat(&mut conn, user.user_id, invoice.flat_id)
        .await
        .map_err(internal_err)?;
    if !owns_flat {
        return Err(forbidden(
            "You can only submit a payment claim for your own flat",
        ));
    }

    let claim_id = accounting::claims::submit_claim(
        &mut conn,
        accounting::claims::SubmitClaimRequest {
            invoice_id: payload.invoice_id,
            claimed_by: user.user_id,
            amount: payload.amount,
            payment_method: &payload.payment_method,
            reference_number: &payload.reference_number,
            note: payload.note.as_deref(),
        },
    )
    .await
    .map_err(internal_err)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(SubmitClaimResponse { claim_id }),
    ))
}

#[derive(serde::Deserialize)]
pub struct ListMyClaimsRequest {
    pub status: Option<String>,
}

pub async fn list_my_claims(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ListMyClaimsRequest>,
) -> Result<Json<Vec<PaymentClaim>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let claims = accounting::db::list_payment_claims(
        &mut conn,
        payload.status.as_deref(),
        Some(user.user_id),
    )
    .await
    .map_err(internal_err)?;
    Ok(Json(claims))
}

#[derive(serde::Deserialize)]
pub struct ListClaimsRequest {
    pub status: Option<String>,
}

/// Admin review queue — all residents' claims, not scoped to one user.
pub async fn list_claims(
    State(state): State<AppState>,
    Json(payload): Json<ListClaimsRequest>,
) -> Result<Json<Vec<PaymentClaim>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let claims = accounting::db::list_payment_claims(&mut conn, payload.status.as_deref(), None)
        .await
        .map_err(internal_err)?;
    Ok(Json(claims))
}

#[derive(serde::Serialize)]
pub struct ConfirmClaimResponse {
    pub payment_id: accounting::PaymentId,
}

pub async fn confirm_claim(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::extract::Path(claim_id): axum::extract::Path<ClaimId>,
) -> Result<Json<ConfirmClaimResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let payment_id = accounting::claims::confirm_claim(&mut tx, claim_id, user.user_id)
        .await
        .map_err(|e| match &e {
            accounting::claims::ClaimError::NotFound => not_found("Claim not found"),
            _ => bad_request(e.to_string()),
        })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(Json(ConfirmClaimResponse { payment_id }))
}

#[derive(serde::Deserialize)]
pub struct RejectClaimRequest {
    pub review_note: String,
}

pub async fn reject_claim(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::extract::Path(claim_id): axum::extract::Path<ClaimId>,
    Json(payload): Json<RejectClaimRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    if payload.review_note.trim().is_empty() {
        return Err(bad_request(
            "A review note is required when rejecting a claim",
        ));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    accounting::claims::reject_claim(&mut tx, claim_id, user.user_id, &payload.review_note)
        .await
        .map_err(|e| match &e {
            accounting::claims::ClaimError::NotFound => not_found("Claim not found"),
            _ => bad_request(e.to_string()),
        })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
