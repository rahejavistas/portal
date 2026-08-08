use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use axum::extract::State;
use chrono::NaiveDate;

use super::csv_response;

fn resolve_as_of(as_of: Option<NaiveDate>) -> NaiveDate {
    as_of.unwrap_or_else(|| chrono::Utc::now().date_naive())
}

fn require_range(
    date_from: Option<NaiveDate>,
    date_to: Option<NaiveDate>,
) -> Result<(NaiveDate, NaiveDate), ApiError> {
    match (date_from, date_to) {
        (Some(f), Some(t)) => Ok((f, t)),
        _ => Err(bad_request(
            "date_from and date_to are required for this report_type",
        )),
    }
}

/// Every CSV report the module produces, dispatched off `report_type` — one route
/// instead of nine near-identical ones. Each variant just picks which parameters it
/// needs from the same request shape and which `accounting::reports::*` query to run;
/// the actual report logic hasn't moved, only how a caller reaches it.
#[derive(serde::Deserialize)]
pub struct ReportRequest {
    pub report_type: String,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    pub as_of: Option<NaiveDate>,
    pub status: Option<String>,
    pub flat_id: Option<i32>,
    pub min_days_overdue: Option<i32>,
}

pub async fn run_report(
    State(state): State<AppState>,
    axum::Json(payload): axum::Json<ReportRequest>,
) -> Result<axum::response::Response, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let as_of = resolve_as_of(payload.as_of);

    match payload.report_type.as_str() {
        "invoice_register" => {
            let (from, to) = require_range(payload.date_from, payload.date_to)?;
            let rows = accounting::reports::invoice_register(
                &mut conn,
                from,
                to,
                payload.status.as_deref(),
                payload.flat_id,
            )
            .await
            .map_err(internal_err)?;
            csv_response("invoice-register.csv", &rows)
        }
        "gst_summary" => {
            let (from, to) = require_range(payload.date_from, payload.date_to)?;
            let rows = accounting::reports::gst_summary(&mut conn, from, to)
                .await
                .map_err(internal_err)?;
            csv_response("gst-summary.csv", &rows)
        }
        "interest" => {
            let (from, to) = require_range(payload.date_from, payload.date_to)?;
            let rows = accounting::reports::interest_report(&mut conn, from, to)
                .await
                .map_err(internal_err)?;
            csv_response("interest-report.csv", &rows)
        }
        "outstanding_dues" => {
            let rows = accounting::reports::outstanding_dues(&mut conn, as_of)
                .await
                .map_err(internal_err)?;
            csv_response("outstanding-dues.csv", &rows)
        }
        "ageing_summary" => {
            let rows = accounting::reports::ageing_summary(&mut conn, as_of)
                .await
                .map_err(internal_err)?;
            csv_response("ageing-summary.csv", &rows)
        }
        "defaulters" => {
            let min_days = payload.min_days_overdue.unwrap_or(90);
            let rows = accounting::reports::defaulters(&mut conn, as_of, min_days)
                .await
                .map_err(internal_err)?;
            csv_response("defaulters.csv", &rows)
        }
        "receipt_register" => {
            let (from, to) = require_range(payload.date_from, payload.date_to)?;
            let rows = accounting::reports::receipt_register(&mut conn, from, to)
                .await
                .map_err(internal_err)?;
            csv_response("receipt-register.csv", &rows)
        }
        "payment_mode_summary" => {
            let (from, to) = require_range(payload.date_from, payload.date_to)?;
            let rows = accounting::reports::payment_mode_summary(&mut conn, from, to)
                .await
                .map_err(internal_err)?;
            csv_response("payment-mode-summary.csv", &rows)
        }
        "income_summary" => {
            let (from, to) = require_range(payload.date_from, payload.date_to)?;
            let rows = accounting::reports::income_summary(&mut conn, from, to)
                .await
                .map_err(internal_err)?;
            csv_response("income-summary.csv", &rows)
        }
        other => Err(bad_request(format!("Unknown report_type '{}'", other))),
    }
}

// collection_summary and preview_bill_generation stay as their own endpoints rather
// than joining the dispatcher above: both return a single JSON object (a dashboard
// stat, a live computation), not a CSV row-dump — a genuinely different response
// shape, not just a different report. Folding them in would mean run_report
// returning sometimes-CSV-sometimes-JSON, which is worse than two extra routes.

#[derive(serde::Deserialize)]
pub struct PeriodRequest {
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
}

pub async fn collection_summary(
    State(state): State<AppState>,
    axum::Json(payload): axum::Json<PeriodRequest>,
) -> Result<axum::Json<accounting::reports::CollectionSummary>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let summary =
        accounting::reports::collection_summary(&mut conn, payload.date_from, payload.date_to)
            .await
            .map_err(internal_err)?;
    Ok(axum::Json(summary))
}

#[derive(serde::Deserialize)]
pub struct PreviewBillingRequest {
    pub period_start: NaiveDate,
}

/// Bill-generation preview — computed live, nothing is written. Lets an admin catch a
/// bad billing-plan config before residents ever see a bill.
pub async fn preview_bill_generation(
    State(state): State<AppState>,
    axum::Json(payload): axum::Json<PreviewBillingRequest>,
) -> Result<axum::Json<Vec<accounting::reports::BillPreviewRow>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rows = accounting::reports::preview_bill_generation(&mut conn, payload.period_start)
        .await
        .map_err(internal_err)?;
    Ok(axum::Json(rows))
}
