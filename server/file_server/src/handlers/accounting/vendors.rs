use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::{PurchaseOrder, Vendor, VendorExpenseListRow};
use accounting::{AccountId, ExpenseId, PoId, VendorId};
use axum::{extract::State, http::StatusCode, Json};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::not_found;

#[derive(serde::Deserialize)]
pub struct CreateVendorRequest {
    pub name: String,
    pub category: String,
    pub gstin: Option<String>,
    pub pan: Option<String>,
}

pub async fn create_vendor(
    State(state): State<AppState>,
    Json(payload): Json<CreateVendorRequest>,
) -> Result<(StatusCode, Json<VendorId>), ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let vendor_id = accounting::db::insert_vendor(
        &mut conn,
        accounting::db::NewVendor {
            name: &payload.name,
            category: &payload.category,
            gstin: payload.gstin.as_deref(),
            pan: payload.pan.as_deref(),
        },
    )
    .await
    .map_err(internal_err)?;
    Ok((StatusCode::CREATED, Json(vendor_id)))
}

pub async fn list_vendors(State(state): State<AppState>) -> Result<Json<Vec<Vendor>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let vendors = accounting::db::list_vendors(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(vendors))
}

#[derive(serde::Deserialize)]
pub struct CreatePurchaseOrderRequest {
    pub vendor_id: VendorId,
    pub description: String,
    pub amount: Decimal,
}

pub async fn create_purchase_order(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<CreatePurchaseOrderRequest>,
) -> Result<(StatusCode, Json<PoId>), ApiError> {
    if payload.amount <= Decimal::ZERO {
        return Err(bad_request("amount must be positive"));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let po_number = accounting::db::next_po_number(&mut conn)
        .await
        .map_err(internal_err)?;
    let po_id = accounting::db::insert_purchase_order(
        &mut conn,
        accounting::db::NewPurchaseOrder {
            po_number: &po_number,
            vendor_id: payload.vendor_id,
            description: &payload.description,
            amount: payload.amount,
            created_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok((StatusCode::CREATED, Json(po_id)))
}

#[derive(serde::Deserialize)]
pub struct ListPurchaseOrdersRequest {
    pub vendor_id: Option<VendorId>,
}

pub async fn list_purchase_orders(
    State(state): State<AppState>,
    Json(payload): Json<ListPurchaseOrdersRequest>,
) -> Result<Json<Vec<PurchaseOrder>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let pos = accounting::db::list_purchase_orders(&mut conn, payload.vendor_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(pos))
}

#[derive(serde::Deserialize)]
pub struct SubmitExpenseRequest {
    pub po_id: Option<PoId>,
    pub vendor_id: VendorId,
    pub account_id: AccountId,
    pub payment_category: String,
    pub vendor_invoice_number: Option<String>,
    pub invoice_document_id: Option<Uuid>,
    pub amount: Decimal,
    /// Invoice date, used to resolve the fiscal year for TDS annual-aggregate tracking.
    pub expense_date: chrono::NaiveDate,
}

pub async fn submit_expense(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SubmitExpenseRequest>,
) -> Result<(StatusCode, Json<ExpenseId>), ApiError> {
    if payload.amount <= Decimal::ZERO {
        return Err(bad_request("amount must be positive"));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let fiscal_year_id = accounting::db::resolve_fiscal_year_id(&mut conn, payload.expense_date)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| bad_request("No fiscal year covers this expense date"))?;

    let expense_id = accounting::vendors::submit_expense(
        &mut conn,
        accounting::vendors::SubmitExpenseRequest {
            po_id: payload.po_id,
            vendor_id: payload.vendor_id,
            account_id: payload.account_id,
            payment_category: &payload.payment_category,
            vendor_invoice_number: payload.vendor_invoice_number.as_deref(),
            invoice_document_id: payload.invoice_document_id,
            amount: payload.amount,
            created_by: user.user_id,
        },
        fiscal_year_id,
    )
    .await
    .map_err(|e| match e {
        accounting::vendors::ExpenseError::PoNotFound => not_found("Purchase order not found"),
        accounting::vendors::ExpenseError::PoVendorMismatch => bad_request(e.to_string()),
        _ => internal_err(e),
    })?;

    Ok((StatusCode::CREATED, Json(expense_id)))
}

#[derive(serde::Deserialize)]
pub struct ListExpensesRequest {
    pub status: Option<String>,
}

pub async fn list_expenses(
    State(state): State<AppState>,
    Json(payload): Json<ListExpensesRequest>,
) -> Result<Json<Vec<VendorExpenseListRow>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let expenses = accounting::db::list_vendor_expenses(&mut conn, payload.status.as_deref())
        .await
        .map_err(internal_err)?;
    Ok(Json(expenses))
}

#[derive(serde::Serialize)]
pub struct ApproveExpenseResponse {
    pub transaction_id: accounting::TransactionId,
}

pub async fn approve_expense(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::extract::Path(expense_id): axum::extract::Path<ExpenseId>,
) -> Result<Json<ApproveExpenseResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let transaction_id = accounting::vendors::approve_expense(
        &mut tx,
        expense_id,
        user.user_id,
        chrono::Utc::now().date_naive(),
    )
    .await
    .map_err(|e| match &e {
        accounting::vendors::ExpenseError::NotFound => not_found("Expense not found"),
        _ => bad_request(e.to_string()),
    })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(Json(ApproveExpenseResponse { transaction_id }))
}

#[derive(serde::Deserialize)]
pub struct RejectExpenseRequest {
    pub reason: String,
}

pub async fn reject_expense(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::extract::Path(expense_id): axum::extract::Path<ExpenseId>,
    Json(payload): Json<RejectExpenseRequest>,
) -> Result<StatusCode, ApiError> {
    if payload.reason.trim().is_empty() {
        return Err(bad_request("A rejection reason is required"));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    accounting::vendors::reject_expense(&mut tx, expense_id, user.user_id, &payload.reason)
        .await
        .map_err(|e| match &e {
            accounting::vendors::ExpenseError::NotFound => not_found("Expense not found"),
            _ => bad_request(e.to_string()),
        })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}
