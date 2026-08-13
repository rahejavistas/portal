use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, forbidden, internal_err, ApiError};
use crate::state::AppState;
use accounting::{InvoiceId, PaymentId};
use axum::extract::State;
use chrono::NaiveDate;

use super::{not_found, pdf_response};

async fn build_invoice_pdf(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<(String, Vec<u8>, i32), ApiError> {
    let invoice = accounting::db::get_invoice(conn, invoice_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Invoice not found"))?;
    let flat = accounting::db::get_flat_identity(conn, invoice.flat_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Flat not found"))?;
    let line_item_rows = accounting::db::get_invoice_line_items(conn, invoice_id)
        .await
        .map_err(internal_err)?;
    let line_items: Vec<accounting::pdf::InvoiceLineItemView> = line_item_rows
        .iter()
        .map(|li| accounting::pdf::InvoiceLineItemView {
            description: &li.description,
            amount: li.amount,
            gst_rate_pct: li.gst_rate_pct,
            hsn_sac_code: li.hsn_sac_code.as_deref(),
            gst_amount: li.gst_amount,
        })
        .collect();
    let society = accounting::db::get_society_profile(conn)
        .await
        .map_err(internal_err)?;
    let gst_rule = accounting::db::get_active_gst_rule(conn, invoice.billing_period_start)
        .await
        .map_err(internal_err)?;
    let interest_rule = accounting::db::get_active_interest_rule(conn, invoice.due_date)
        .await
        .map_err(internal_err)?;
    let instructions = accounting::db::get_payment_instructions(conn)
        .await
        .map_err(internal_err)?;

    let bytes = accounting::pdf::render_invoice_pdf(
        &invoice,
        &line_items,
        &flat.wing,
        &flat.flat_number,
        &society,
        gst_rule.as_ref().and_then(|r| r.society_gstin.as_deref()),
        &instructions,
        interest_rule.map(|r| r.annual_rate_pct),
        false,
    );
    Ok((
        format!("{}.pdf", invoice.invoice_number),
        bytes,
        invoice.flat_id,
    ))
}

async fn build_receipt_pdf(
    conn: &mut sqlx::PgConnection,
    payment_id: PaymentId,
) -> Result<(String, Vec<u8>, i32), ApiError> {
    let payment = accounting::db::get_payment(conn, payment_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Payment not found"))?;
    let receipt = accounting::db::get_receipt_by_payment(conn, payment_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Receipt not found for this payment"))?;
    let invoice = accounting::db::get_invoice(conn, payment.invoice_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Invoice not found"))?;
    let flat = accounting::db::get_flat_identity(conn, invoice.flat_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Flat not found"))?;

    let bytes = accounting::pdf::render_receipt_pdf(
        &receipt.receipt_number,
        &payment,
        &invoice.invoice_number,
        &flat.wing,
        &flat.flat_number,
    );
    Ok((
        format!("{}.pdf", receipt.receipt_number),
        bytes,
        invoice.flat_id,
    ))
}

async fn build_statement_pdf(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<u8>, ApiError> {
    let flat = accounting::db::get_flat_identity(conn, flat_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Flat not found"))?;
    let rows = accounting::reports::member_statement(conn, flat_id, date_from, date_to)
        .await
        .map_err(internal_err)?;
    let pdf_rows: Vec<accounting::pdf::StatementRow> = rows
        .into_iter()
        .map(|r| accounting::pdf::StatementRow {
            date: r.transaction_date,
            description: r.description,
            debit: r.debit,
            credit: r.credit,
            running_balance: r.running_balance,
        })
        .collect();
    let period_label = format!(
        "{} to {}",
        date_from.format("%d %b %Y"),
        date_to.format("%d %b %Y")
    );
    Ok(accounting::pdf::render_statement_pdf(
        &flat.wing,
        &flat.flat_number,
        &period_label,
        &pdf_rows,
    ))
}

async fn build_noc_pdf(conn: &mut sqlx::PgConnection, flat_id: i32) -> Result<Vec<u8>, ApiError> {
    let flat = accounting::db::get_flat_identity(conn, flat_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Flat not found"))?;

    if accounting::db::has_outstanding_dues(conn, flat_id)
        .await
        .map_err(internal_err)?
    {
        return Err(bad_request(
            "Cannot issue a NOC — this flat has outstanding dues",
        ));
    }

    let owner_email = accounting::db::get_flat_owner_email(conn, flat_id)
        .await
        .map_err(internal_err)?;

    Ok(accounting::pdf::render_noc_pdf(
        &flat.wing,
        &flat.flat_number,
        owner_email.as_deref(),
        chrono::Utc::now().date_naive(),
    ))
}

/// Every generated PDF, dispatched off `kind` — one route for both the admin (no
/// ownership check) and member (must own the flat) variants.
#[derive(serde::Deserialize)]
pub struct PdfRequest {
    pub kind: String,
    pub invoice_id: Option<InvoiceId>,
    pub payment_id: Option<PaymentId>,
    pub flat_id: Option<i32>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

fn require_flat_id(payload: &PdfRequest) -> Result<i32, ApiError> {
    payload
        .flat_id
        .ok_or_else(|| bad_request("flat_id is required for this kind"))
}

/// Returns the rendered PDF plus the flat_id it belongs to, for the member-facing ownership check.
async fn dispatch_pdf(
    conn: &mut sqlx::PgConnection,
    payload: &PdfRequest,
) -> Result<(String, Vec<u8>, i32), ApiError> {
    match payload.kind.as_str() {
        "invoice" => {
            let invoice_id = payload
                .invoice_id
                .ok_or_else(|| bad_request("invoice_id is required for kind=invoice"))?;
            build_invoice_pdf(conn, invoice_id).await
        }
        "receipt" => {
            let payment_id = payload
                .payment_id
                .ok_or_else(|| bad_request("payment_id is required for kind=receipt"))?;
            build_receipt_pdf(conn, payment_id).await
        }
        "statement" => {
            let flat_id = require_flat_id(payload)?;
            let (from, to) = match (payload.date_from, payload.date_to) {
                (Some(f), Some(t)) => (f, t),
                _ => {
                    return Err(bad_request(
                        "date_from and date_to are required for kind=statement",
                    ))
                }
            };
            let bytes = build_statement_pdf(conn, flat_id, from, to).await?;
            Ok(("statement.pdf".to_string(), bytes, flat_id))
        }
        "noc" => {
            let flat_id = require_flat_id(payload)?;
            let bytes = build_noc_pdf(conn, flat_id).await?;
            Ok(("noc.pdf".to_string(), bytes, flat_id))
        }
        "sample_invoice" => {
            let today = chrono::Utc::now().date_naive();
            let gst_rule = accounting::db::get_active_gst_rule(conn, today)
                .await
                .map_err(internal_err)?;
            let society = accounting::db::get_society_profile(conn)
                .await
                .map_err(internal_err)?;
            let interest_rule = accounting::db::get_active_interest_rule(conn, today)
                .await
                .map_err(internal_err)?;
            let instructions = accounting::db::get_payment_instructions(conn)
                .await
                .map_err(internal_err)?;
            let bytes = accounting::pdf::render_sample_invoice_pdf(
                &gst_rule,
                &society,
                &instructions,
                interest_rule.map(|r| r.annual_rate_pct),
            );
            Ok(("sample-invoice.pdf".to_string(), bytes, 0))
        }
        "sample_receipt" => {
            let bytes = accounting::pdf::render_sample_receipt_pdf();
            Ok(("sample-receipt.pdf".to_string(), bytes, 0))
        }
        other => Err(bad_request(format!("Unknown kind '{}'", other))),
    }
}

pub async fn admin_document_pdf(
    State(state): State<AppState>,
    axum::Json(payload): axum::Json<PdfRequest>,
) -> Result<axum::response::Response, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let (filename, bytes, _flat_id) = dispatch_pdf(&mut conn, &payload).await?;
    Ok(pdf_response(&filename, bytes))
}

/// One generic error for every way a member's PDF request can fail — "doesn't exist"
/// and "not yours" must look identical, or the error leaks which ids are real.
fn not_yours() -> ApiError {
    forbidden("This document does not belong to your flat")
}

/// Resolves which flat a PDF request is about, without running any business logic
/// that could itself leak info (e.g. the NOC dues check) before ownership is checked.
async fn resolve_flat_id_for_pdf(
    conn: &mut sqlx::PgConnection,
    payload: &PdfRequest,
) -> Result<i32, ApiError> {
    match payload.kind.as_str() {
        "invoice" => {
            let invoice_id = payload.invoice_id.ok_or_else(not_yours)?;
            let invoice = accounting::db::get_invoice(conn, invoice_id)
                .await
                .map_err(internal_err)?
                .ok_or_else(not_yours)?;
            Ok(invoice.flat_id)
        }
        "receipt" => {
            let payment_id = payload.payment_id.ok_or_else(not_yours)?;
            let payment = accounting::db::get_payment(conn, payment_id)
                .await
                .map_err(internal_err)?
                .ok_or_else(not_yours)?;
            let invoice = accounting::db::get_invoice(conn, payment.invoice_id)
                .await
                .map_err(internal_err)?
                .ok_or_else(not_yours)?;
            Ok(invoice.flat_id)
        }
        "statement" | "noc" => payload.flat_id.ok_or_else(not_yours),
        // Not tied to any flat — flat_id 0 never exists, so ownership always fails.
        "sample_invoice" | "sample_receipt" => Ok(0),
        _ => Err(not_yours()),
    }
}

pub async fn member_document_pdf(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::Json(payload): axum::Json<PdfRequest>,
) -> Result<axum::response::Response, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let flat_id = resolve_flat_id_for_pdf(&mut conn, &payload).await?;
    if !accounting::db::user_owns_flat(&mut conn, user.user_id, flat_id)
        .await
        .map_err(internal_err)?
    {
        return Err(not_yours());
    }
    let (filename, bytes, _flat_id) = dispatch_pdf(&mut conn, &payload).await?;
    Ok(pdf_response(&filename, bytes))
}
