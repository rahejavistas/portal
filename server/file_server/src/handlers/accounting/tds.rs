use crate::extractors::SessionUser;
use crate::handlers::error::{internal_err, ApiError};
use crate::state::AppState;
use accounting::db::{TdsRule, TdsSummaryRow};
use accounting::{FiscalYearId, VendorId};
use axum::{extract::State, http::StatusCode, Json};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(serde::Deserialize)]
pub struct CreateTdsRuleRequest {
    pub section: String,
    pub payment_category: String,
    pub rate_pct: Decimal,
    pub threshold_single_payment: Decimal,
    pub threshold_annual_aggregate: Decimal,
    pub effective_from: NaiveDate,
}

pub async fn create_tds_rule(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateTdsRuleRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    accounting::db::insert_tds_rule(
        &mut conn,
        accounting::db::NewTdsRule {
            section: &payload.section,
            payment_category: &payload.payment_category,
            rate_pct: payload.rate_pct,
            threshold_single_payment: payload.threshold_single_payment,
            threshold_annual_aggregate: payload.threshold_annual_aggregate,
            effective_from: payload.effective_from,
            created_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok(StatusCode::CREATED)
}

pub async fn list_tds_rules(State(state): State<AppState>) -> Result<Json<Vec<TdsRule>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rules = accounting::db::list_active_tds_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(rules))
}

/// Form 16A source data for one vendor's fiscal year — never e-files, only assembles
/// the numbers a CA needs to file it.
#[derive(serde::Deserialize)]
pub struct TdsSummaryRequest {
    pub vendor_id: VendorId,
    pub fiscal_year_id: FiscalYearId,
}

pub async fn tds_summary(
    State(state): State<AppState>,
    Json(payload): Json<TdsSummaryRequest>,
) -> Result<Json<Vec<TdsSummaryRow>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rows = accounting::db::tds_summary_for_vendor(
        &mut conn,
        payload.vendor_id,
        payload.fiscal_year_id,
    )
    .await
    .map_err(internal_err)?;
    Ok(Json(rows))
}
