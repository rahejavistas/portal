use crate::handlers::error::{internal_err, ApiError};
use crate::state::AppState;
use accounting::db::{Fund, FundContributionRule};
use accounting::{FundId, FundRuleId};
use axum::{extract::State, http::StatusCode, Json};
use chrono::NaiveDate;
use rust_decimal::Decimal;

pub async fn list_funds(State(state): State<AppState>) -> Result<Json<Vec<Fund>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let funds = accounting::db::list_funds(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(funds))
}

#[derive(serde::Deserialize)]
pub struct CreateFundContributionRuleRequest {
    pub fund_id: FundId,
    pub contribution_type: String,
    pub amount_or_pct: Decimal,
    pub effective_from: NaiveDate,
}

pub async fn create_fund_contribution_rule(
    State(state): State<AppState>,
    Json(payload): Json<CreateFundContributionRuleRequest>,
) -> Result<(StatusCode, Json<FundRuleId>), ApiError> {
    if !["FIXED", "PCT_OF_MAINTENANCE"].contains(&payload.contribution_type.as_str()) {
        return Err(crate::handlers::error::bad_request(
            "contribution_type must be FIXED or PCT_OF_MAINTENANCE",
        ));
    }
    if payload.amount_or_pct < Decimal::ZERO {
        return Err(crate::handlers::error::bad_request(
            "amount_or_pct cannot be negative",
        ));
    }
    if payload.contribution_type == "PCT_OF_MAINTENANCE"
        && payload.amount_or_pct > Decimal::from(100)
    {
        return Err(crate::handlers::error::bad_request(
            "amount_or_pct cannot exceed 100 for PCT_OF_MAINTENANCE",
        ));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rule_id = accounting::db::insert_fund_contribution_rule(
        &mut conn,
        accounting::db::NewFundContributionRule {
            fund_id: payload.fund_id,
            contribution_type: payload.contribution_type,
            amount_or_pct: payload.amount_or_pct,
            effective_from: payload.effective_from,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok((StatusCode::CREATED, Json(rule_id)))
}

#[derive(serde::Deserialize)]
pub struct AssignFlatToFundRequest {
    pub flat_id: i32,
    pub fund_id: FundId,
}

pub async fn assign_flat_to_fund(
    State(state): State<AppState>,
    Json(payload): Json<AssignFlatToFundRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    accounting::db::assign_flat_to_fund(&mut conn, payload.flat_id, payload.fund_id)
        .await
        .map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
pub struct ListFundRulesForFlatRequest {
    pub flat_id: i32,
    pub as_of: Option<NaiveDate>,
}

pub async fn list_fund_rules_for_flat(
    State(state): State<AppState>,
    Json(payload): Json<ListFundRulesForFlatRequest>,
) -> Result<Json<Vec<FundContributionRule>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let as_of = payload
        .as_of
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let rules =
        accounting::db::get_fund_contribution_rules_for_flat(&mut conn, payload.flat_id, as_of)
            .await
            .map_err(internal_err)?;
    Ok(Json(rules))
}
