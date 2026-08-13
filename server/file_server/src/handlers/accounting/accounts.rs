use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::Account;
use accounting::AccountId;
use axum::{extract::State, http::StatusCode, Json};

/// Dynamic chart-of-accounts management — lets an admin (or an AI ingesting a bylaws
/// document, see `settings::overview`) add a new income/expense head the system
/// didn't come seeded with (e.g. "Water Charges", "DG Fuel", "Lift AMC") without a
/// migration. Deliberately restricted to INCOME/EXPENSE: asset/liability/equity
/// accounts are the ledger's own structural plumbing (bank, receivable, payables,
/// retained earnings) and must never be created ad hoc — creating one wrong would
/// silently corrupt the balance sheet, not just mis-categorize a bill.
#[derive(serde::Deserialize)]
pub struct CreateAccountRequest {
    pub code: String,
    pub name: String,
    pub account_type: String,
}

pub async fn create_account(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<AccountId>), ApiError> {
    if !["INCOME", "EXPENSE"].contains(&payload.account_type.as_str()) {
        return Err(bad_request(
            "account_type must be INCOME or EXPENSE — asset/liability/equity accounts are structural and can't be created here",
        ));
    }
    if payload.code.trim().is_empty() || payload.name.trim().is_empty() {
        return Err(bad_request("code and name are required"));
    }

    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let account_id = accounting::db::insert_account(
        &mut conn,
        payload.code.trim(),
        payload.name.trim(),
        &payload.account_type,
        user.user_id,
    )
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            bad_request(format!("Account code '{}' is already in use", payload.code))
        }
        _ => internal_err(e),
    })?;

    Ok((StatusCode::CREATED, Json(account_id)))
}

#[derive(serde::Deserialize)]
pub struct ListAccountsRequest {
    pub account_type: Option<String>,
    #[serde(default)]
    pub active_only: bool,
}

pub async fn list_accounts(
    State(state): State<AppState>,
    Json(payload): Json<ListAccountsRequest>,
) -> Result<Json<Vec<Account>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let accounts = accounting::db::list_accounts(
        &mut conn,
        payload.account_type.as_deref(),
        payload.active_only,
    )
    .await
    .map_err(internal_err)?;
    Ok(Json(accounts))
}

#[derive(serde::Deserialize)]
pub struct SetAccountActiveRequest {
    pub is_active: bool,
}

pub async fn set_account_active(
    State(state): State<AppState>,
    axum::extract::Path(account_id): axum::extract::Path<AccountId>,
    Json(payload): Json<SetAccountActiveRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let changed = accounting::db::set_account_active(&mut conn, account_id, payload.is_active)
        .await
        .map_err(internal_err)?;
    if !changed {
        return Err(bad_request(
            "Account not found, or it's a protected system account that can't be deactivated",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
