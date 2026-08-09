use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::{BillingPlan, GstRule, InterestRule, Invoice, InvoiceListRow};
use accounting::{BillingPlanId, InvoiceId, PaymentId};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::not_found;

// ---------------------------------------------------------------------------
// Billing plans
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct CreateBillingPlanRequest {
    pub name: String,
    pub billing_type: String,
    pub config: serde_json::Value,
    pub frequency: Option<String>,
}

pub async fn create_billing_plan(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateBillingPlanRequest>,
) -> Result<(StatusCode, Json<BillingPlanId>), ApiError> {
    if !["FLAT_RATE", "PER_SQFT", "SLAB"].contains(&payload.billing_type.as_str()) {
        return Err(bad_request(
            "billing_type must be FLAT_RATE, PER_SQFT, or SLAB",
        ));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let billing_plan_id = accounting::db::insert_billing_plan(
        &mut conn,
        accounting::db::NewBillingPlan {
            name: &payload.name,
            billing_type: &payload.billing_type,
            config: &payload.config,
            frequency: payload.frequency.as_deref(),
            created_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;

    Ok((StatusCode::CREATED, Json(billing_plan_id)))
}

pub async fn list_billing_plans(
    State(state): State<AppState>,
) -> Result<Json<Vec<BillingPlan>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let plans = accounting::db::list_billing_plans(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(plans))
}

#[derive(serde::Deserialize)]
pub struct AssignBillingPlanRequest {
    pub flat_id: i32,
    pub billing_plan_id: BillingPlanId,
    pub area_sqft: Option<Decimal>,
}

pub async fn assign_billing_plan(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AssignBillingPlanRequest>,
) -> Result<StatusCode, ApiError> {
    if payload.area_sqft.is_some_and(|a| a <= Decimal::ZERO) {
        return Err(bad_request("area_sqft must be positive"));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    accounting::db::upsert_flat_billing_assignment(
        &mut tx,
        payload.flat_id,
        payload.billing_plan_id,
        payload.area_sqft,
        user.user_id,
    )
    .await
    .map_err(internal_err)?;
    tx.commit().await.map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Every flat and whatever billing plan it currently has assigned, if any.
pub async fn list_billing_assignments(
    State(state): State<AppState>,
) -> Result<Json<Vec<accounting::db::FlatBillingAssignmentRow>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rows = accounting::db::list_all_flat_billing_assignments(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(rows))
}

#[derive(serde::Deserialize)]
pub struct BulkAssignBillingPlanRequest {
    pub billing_plan_id: BillingPlanId,
}

#[derive(serde::Serialize)]
pub struct BulkAssignBillingPlanResponse {
    pub flats_assigned: u64,
}

/// Assigns a FLAT_RATE plan to every unassigned flat in one batch. PER_SQFT/SLAB plans
/// need a per-flat area, so those still go through `assign_billing_plan`.
pub async fn bulk_assign_billing_plan(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<BulkAssignBillingPlanRequest>,
) -> Result<Json<BulkAssignBillingPlanResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let plan = accounting::db::get_billing_plan(&mut conn, payload.billing_plan_id)
        .await
        .map_err(|_| not_found("Billing plan not found"))?;
    if plan.billing_type != "FLAT_RATE" {
        return Err(bad_request(
            "Only FLAT_RATE plans can be bulk-assigned — PER_SQFT and SLAB plans need an area_sqft entered per flat",
        ));
    }
    let flats_assigned = accounting::db::bulk_assign_flat_rate_plan_to_unassigned(
        &mut conn,
        payload.billing_plan_id,
        user.user_id,
    )
    .await
    .map_err(internal_err)?;
    Ok(Json(BulkAssignBillingPlanResponse { flats_assigned }))
}

// ---------------------------------------------------------------------------
// Invoices
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct GenerateInvoicesRequest {
    /// If set, bills only this flat; otherwise every flat with a billing assignment.
    pub flat_id: Option<i32>,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub due_date: NaiveDate,
}

#[derive(serde::Serialize)]
pub struct GenerateInvoicesResponse {
    pub invoices_created: Vec<InvoiceId>,
    /// flat_id -> reason, for flats that couldn't be billed (e.g. already billed, no plan).
    pub skipped: Vec<(i32, String)>,
}

pub async fn generate_invoices(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<GenerateInvoicesRequest>,
) -> Result<Json<GenerateInvoicesResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;

    let flat_ids: Vec<i32> = match payload.flat_id {
        Some(id) => vec![id],
        None => accounting::db::list_flats_with_billing_assignment(&mut conn)
            .await
            .map_err(internal_err)?,
    };

    let mut invoices_created = Vec::new();
    let mut skipped = Vec::new();
    for flat_id in flat_ids {
        let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
        match accounting::billing::generate_invoice_for_flat(
            &mut tx,
            flat_id,
            payload.period_start,
            payload.period_end,
            payload.due_date,
            user.user_id,
        )
        .await
        {
            Ok(invoice_id) => {
                tx.commit().await.map_err(internal_err)?;
                invoices_created.push(invoice_id);
            }
            Err(e) => {
                tx.rollback().await.map_err(internal_err)?;
                skipped.push((flat_id, e.to_string()));
            }
        }
    }

    Ok(Json(GenerateInvoicesResponse {
        invoices_created,
        skipped,
    }))
}

#[derive(serde::Deserialize)]
pub struct ListInvoicesRequest {
    pub flat_id: Option<i32>,
    pub status: Option<String>,
}

pub async fn list_invoices(
    State(state): State<AppState>,
    Json(payload): Json<ListInvoicesRequest>,
) -> Result<Json<Vec<InvoiceListRow>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let invoices =
        accounting::db::list_invoices(&mut conn, payload.flat_id, payload.status.as_deref())
            .await
            .map_err(internal_err)?;
    Ok(Json(invoices))
}

/// Every invoice for a flat the caller owns — never takes a flat_id from the request.
pub async fn list_my_invoices(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Invoice>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let flat_ids = user
        .get_active_flat_ids(&mut *conn)
        .await
        .map_err(internal_err)?;
    let invoices = accounting::db::list_invoices_for_flats(&mut conn, &flat_ids)
        .await
        .map_err(internal_err)?;
    Ok(Json(invoices))
}

#[derive(serde::Deserialize)]
pub struct VoidInvoiceRequest {
    pub reason: String,
}

pub async fn void_invoice(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Path(invoice_id): Path<InvoiceId>,
    Json(payload): Json<VoidInvoiceRequest>,
) -> Result<StatusCode, ApiError> {
    if payload.reason.trim().is_empty() {
        return Err(bad_request("A void reason is required"));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let invoice = accounting::db::get_invoice_for_update(&mut tx, invoice_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Invoice not found"))?;
    if invoice.status == "VOID" {
        return Err(bad_request("Invoice is already void"));
    }
    if invoice.amount_paid > Decimal::ZERO {
        return Err(bad_request(
            "Cannot void an invoice that already has a payment recorded against it",
        ));
    }
    accounting::billing::void_invoice(
        &mut tx,
        invoice_id,
        &payload.reason,
        chrono::Utc::now().date_naive(),
        user.user_id,
    )
    .await
    .map_err(internal_err)?;
    tx.commit().await.map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GST & interest rules
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct CreateGstRuleRequest {
    pub per_member_monthly_exempt_threshold: Decimal,
    pub society_turnover_registration_threshold: Decimal,
    pub gst_rate_pct: Decimal,
    pub hsn_sac_code: String,
    pub society_gstin: Option<String>,
    pub effective_from: NaiveDate,
}

pub async fn create_gst_rule(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateGstRuleRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    accounting::db::insert_gst_rule(
        &mut conn,
        accounting::db::NewGstRule {
            per_member_monthly_exempt_threshold: payload.per_member_monthly_exempt_threshold,
            society_turnover_registration_threshold: payload
                .society_turnover_registration_threshold,
            gst_rate_pct: payload.gst_rate_pct,
            hsn_sac_code: &payload.hsn_sac_code,
            society_gstin: payload.society_gstin.as_deref(),
            effective_from: payload.effective_from,
            created_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok(StatusCode::CREATED)
}

pub async fn list_gst_rules(State(state): State<AppState>) -> Result<Json<Vec<GstRule>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rules = accounting::db::list_active_gst_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(rules))
}

#[derive(serde::Deserialize)]
pub struct CreateInterestRuleRequest {
    pub annual_rate_pct: Decimal,
    pub grace_period_days: i32,
    pub compounding: String,
    pub effective_from: NaiveDate,
}

pub async fn create_interest_rule(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateInterestRuleRequest>,
) -> Result<StatusCode, ApiError> {
    if !["SIMPLE", "MONTHLY"].contains(&payload.compounding.as_str()) {
        return Err(bad_request("compounding must be SIMPLE or MONTHLY"));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    accounting::db::insert_interest_rule(
        &mut conn,
        accounting::db::NewInterestRule {
            annual_rate_pct: payload.annual_rate_pct,
            grace_period_days: payload.grace_period_days,
            compounding: payload.compounding,
            effective_from: payload.effective_from,
            created_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok(StatusCode::CREATED)
}

pub async fn list_interest_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<InterestRule>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rules = accounting::db::list_active_interest_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(rules))
}

#[derive(serde::Serialize)]
pub struct RunInterestResponse {
    pub charges_posted: usize,
}

/// Meant to be hit by an external trigger (Cloud Scheduler, or an admin button).
pub async fn run_interest(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<RunInterestResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let charges_posted = accounting::billing::run_interest_for_all_overdue(
        &mut tx,
        chrono::Utc::now().date_naive(),
        user.user_id,
    )
    .await
    .map_err(internal_err)?;
    tx.commit().await.map_err(internal_err)?;
    Ok(Json(RunInterestResponse { charges_posted }))
}

// ---------------------------------------------------------------------------
// Payments & receipts
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct RecordPaymentRequest {
    pub invoice_id: InvoiceId,
    pub amount: Decimal,
    pub payment_method: String,
    pub reference_number: Option<String>,
    pub payment_date: NaiveDate,
}

#[derive(serde::Serialize)]
pub struct RecordPaymentResponse {
    pub payment_id: PaymentId,
    pub receipt_number: String,
    pub newly_recorded: bool,
}

pub async fn record_payment(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<RecordPaymentRequest>,
) -> Result<Json<RecordPaymentResponse>, ApiError> {
    if payload.amount <= Decimal::ZERO {
        return Err(bad_request("amount must be positive"));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let result = accounting::payments::record_payment(
        &mut tx,
        accounting::payments::RecordPaymentRequest {
            invoice_id: payload.invoice_id,
            amount: payload.amount,
            payment_method: &payload.payment_method,
            reference_number: payload.reference_number.as_deref(),
            payment_date: payload.payment_date,
            recorded_by: user.user_id,
        },
    )
    .await
    .map_err(|e| match &e {
        accounting::payments::PaymentError::InvoiceNotFound => not_found("Invoice not found"),
        _ => bad_request(e.to_string()),
    })?;
    tx.commit().await.map_err(internal_err)?;

    Ok(Json(RecordPaymentResponse {
        payment_id: result.payment_id,
        receipt_number: result.receipt_number,
        newly_recorded: result.newly_recorded,
    }))
}

#[derive(serde::Deserialize)]
pub struct ListPaymentsRequest {
    pub invoice_id: Option<InvoiceId>,
    pub flat_id: Option<i32>,
}

pub async fn list_payments(
    State(state): State<AppState>,
    Json(payload): Json<ListPaymentsRequest>,
) -> Result<Json<Vec<accounting::db::PaymentWithReceipt>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let payments = accounting::db::list_payments(&mut conn, payload.invoice_id, payload.flat_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(payments))
}

#[derive(serde::Deserialize)]
pub struct FlatCreditBalanceRequest {
    pub flat_id: i32,
}

pub async fn get_flat_credit_balance(
    State(state): State<AppState>,
    Json(payload): Json<FlatCreditBalanceRequest>,
) -> Result<Json<Decimal>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let balance = accounting::db::get_flat_credit_balance(&mut conn, payload.flat_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(balance))
}

#[derive(serde::Deserialize)]
pub struct ApplyCreditRequest {
    pub invoice_id: InvoiceId,
    pub amount: Decimal,
}

#[derive(serde::Serialize)]
pub struct ApplyCreditResponse {
    pub transaction_id: accounting::TransactionId,
}

pub async fn apply_credit_to_invoice(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ApplyCreditRequest>,
) -> Result<Json<ApplyCreditResponse>, ApiError> {
    if payload.amount <= Decimal::ZERO {
        return Err(bad_request("amount must be positive"));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let transaction_id = accounting::payments::apply_credit_to_invoice(
        &mut tx,
        payload.invoice_id,
        payload.amount,
        user.user_id,
        chrono::Utc::now().date_naive(),
    )
    .await
    .map_err(|e| match &e {
        accounting::payments::CreditApplicationError::InvoiceNotFound => {
            not_found("Invoice not found")
        }
        _ => bad_request(e.to_string()),
    })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(Json(ApplyCreditResponse { transaction_id }))
}
