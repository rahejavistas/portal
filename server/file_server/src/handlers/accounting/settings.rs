use crate::handlers::error::{internal_err, ApiError};
use crate::state::AppState;
use axum::{extract::State, Json};

/// Every configurable "lever" in the accounting module, grouped by category, in one
/// call — the read-side an AI extracting numbers from a bylaws document (or an admin
/// settings UI) targets to see current state before proposing changes. Each category
/// still has its own dedicated create/set endpoint to actually change it (GST rules
/// via `/gst-rules/create`, TDS via `/tds-rules/create`, etc.) — this endpoint is
/// intentionally read-only, so applying a bylaws-derived change always goes through
/// the same validated path a human admin would use, never a bulk blind write.
#[derive(serde::Serialize)]
pub struct SettingsOverview {
    pub gst: Vec<accounting::db::GstRule>,
    pub interest: Vec<accounting::db::InterestRule>,
    pub tds: Vec<accounting::db::TdsRule>,
    pub funds: Vec<accounting::db::Fund>,
    pub fund_contribution_rules: Vec<accounting::db::FundContributionRule>,
    pub billing_plans: Vec<accounting::db::BillingPlan>,
    pub chart_of_accounts: Vec<accounting::db::Account>,
    pub payment_instructions: Option<accounting::db::PaymentInstructions>,
}

pub async fn overview(State(state): State<AppState>) -> Result<Json<SettingsOverview>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;

    let gst = accounting::db::list_active_gst_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    let interest = accounting::db::list_active_interest_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    let tds = accounting::db::list_active_tds_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    let funds = accounting::db::list_funds(&mut conn)
        .await
        .map_err(internal_err)?;
    let fund_contribution_rules = accounting::db::list_all_fund_contribution_rules(&mut conn)
        .await
        .map_err(internal_err)?;
    let billing_plans = accounting::db::list_billing_plans(&mut conn)
        .await
        .map_err(internal_err)?;
    let chart_of_accounts = accounting::db::list_accounts(&mut conn, None, true)
        .await
        .map_err(internal_err)?;
    let payment_instructions = accounting::db::get_payment_instructions(&mut conn)
        .await
        .map_err(internal_err)?;

    Ok(Json(SettingsOverview {
        gst,
        interest,
        tds,
        funds,
        fund_contribution_rules,
        billing_plans,
        chart_of_accounts,
        payment_instructions,
    }))
}
