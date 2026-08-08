use crate::extractors::SessionUser;
use crate::handlers::error::{internal_err, ApiError};
use crate::state::AppState;
use accounting::budgets::VarianceRow;
use accounting::{AccountId, FiscalYearId};
use axum::{extract::State, http::StatusCode, Json};
use rust_decimal::Decimal;

#[derive(serde::Deserialize)]
pub struct SetBudgetRequest {
    pub fiscal_year_id: FiscalYearId,
    pub lines: Vec<BudgetLineInput>,
}

#[derive(serde::Deserialize)]
pub struct BudgetLineInput {
    pub account_id: AccountId,
    pub budgeted_amount: Decimal,
}

pub async fn set_budget(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SetBudgetRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let budget_id =
        accounting::db::get_or_create_budget(&mut conn, payload.fiscal_year_id, user.user_id)
            .await
            .map_err(internal_err)?;
    let account_ids: Vec<AccountId> = payload.lines.iter().map(|l| l.account_id).collect();
    let budgeted_amounts: Vec<Decimal> = payload.lines.iter().map(|l| l.budgeted_amount).collect();
    accounting::db::set_budget_lines(&mut conn, budget_id, &account_ids, &budgeted_amounts)
        .await
        .map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
pub struct VarianceRequest {
    pub fiscal_year_id: FiscalYearId,
}

pub async fn variance_report(
    State(state): State<AppState>,
    Json(payload): Json<VarianceRequest>,
) -> Result<Json<Vec<VarianceRow>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rows = accounting::db::budget_variance(&mut conn, payload.fiscal_year_id)
        .await
        .map_err(internal_err)?
        .into_iter()
        .map(accounting::budgets::compute_variance)
        .collect();
    Ok(Json(rows))
}
