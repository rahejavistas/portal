//! Accounting endpoints: `billing` (plans/invoices/GST/interest rules/payments/credit
//! balance), `claims` (resident payment-claim reconciliation), `reports` (CSV exports),
//! `documents` (invoice/receipt/statement/NOC PDFs), `config` (bank details) — P0.
//! `funds`, `tds`, `vendors`, `bank_reconciliation`, `budgets`, `fiscal_year` — P1.
//! `accounts` (dynamic chart-of-accounts income/expense heads), `settings` (grouped
//! read-only overview of every configurable lever, for bylaws-driven config).

mod accounts;
mod bank_reconciliation;
mod billing;
mod budgets;
mod claims;
mod config;
mod documents;
mod fiscal_year;
mod flats;
mod funds;
mod reports;
mod settings;
mod tds;
mod vendors;

pub use accounts::*;
pub use bank_reconciliation::*;
pub use billing::*;
pub use budgets::*;
pub use claims::*;
pub use config::*;
pub use documents::*;
pub use fiscal_year::*;
pub use flats::*;
pub use funds::*;
pub use reports::*;
pub use settings::*;
pub use tds::*;
pub use vendors::*;

use crate::handlers::error::{internal_err, ApiError};
use axum::{http::StatusCode, response::IntoResponse, Json};

pub(super) fn not_found(msg: &str) -> ApiError {
    (StatusCode::NOT_FOUND, Json(msg.to_string()))
}

/// Serializes any list of `Serialize` rows to a downloadable CSV response — every
/// report handler in `reports.rs` funnels through this instead of hand-rolling
/// Content-Type/Content-Disposition headers each time.
pub(super) fn csv_response<T: serde::Serialize>(
    filename: &str,
    rows: &[T],
) -> Result<axum::response::Response, ApiError> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer.serialize(row).map_err(internal_err)?;
    }
    let bytes = writer.into_inner().map_err(internal_err)?;

    Ok((
        StatusCode::OK,
        [
            ("Content-Type", "text/csv".to_string()),
            (
                "Content-Disposition",
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub(crate) fn pdf_response(filename: &str, bytes: Vec<u8>) -> axum::response::Response {
    (
        StatusCode::OK,
        [
            ("Content-Type", "application/pdf".to_string()),
            (
                "Content-Disposition",
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        bytes,
    )
        .into_response()
}
