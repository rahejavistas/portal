use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::FiscalYear;
use accounting::FiscalYearId;
use axum::{extract::State, Json};

use super::not_found;

pub async fn list_fiscal_years(
    State(state): State<AppState>,
) -> Result<Json<Vec<FiscalYear>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let years = accounting::db::list_fiscal_years(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(years))
}

#[derive(serde::Serialize)]
pub struct CloseFiscalYearResponse {
    pub closing_transaction_id: Option<accounting::TransactionId>,
}

pub async fn close_fiscal_year(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::extract::Path(fiscal_year_id): axum::extract::Path<FiscalYearId>,
) -> Result<Json<CloseFiscalYearResponse>, ApiError> {
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let closing_transaction_id =
        accounting::fiscal_year::close_fiscal_year(&mut tx, fiscal_year_id, user.user_id)
            .await
            .map_err(|e| match &e {
                accounting::fiscal_year::FiscalYearError::NotFound => {
                    not_found("Fiscal year not found")
                }
                _ => bad_request(e.to_string()),
            })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(Json(CloseFiscalYearResponse {
        closing_transaction_id,
    }))
}

#[derive(serde::Deserialize)]
pub struct ReopenFiscalYearRequest {
    pub reason: String,
}

pub async fn reopen_fiscal_year(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    axum::extract::Path(fiscal_year_id): axum::extract::Path<FiscalYearId>,
    Json(payload): Json<ReopenFiscalYearRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    if payload.reason.trim().is_empty() {
        return Err(bad_request("A reason is required to reopen a fiscal year"));
    }
    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    accounting::fiscal_year::reopen_fiscal_year(
        &mut tx,
        fiscal_year_id,
        user.user_id,
        &payload.reason,
    )
    .await
    .map_err(|e| match &e {
        accounting::fiscal_year::FiscalYearError::NotFound => not_found("Fiscal year not found"),
        _ => bad_request(e.to_string()),
    })?;
    tx.commit().await.map_err(internal_err)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
