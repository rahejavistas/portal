use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, forbidden, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::{PaymentInstructions, SocietyProfile};
use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, Utc};

/// Society identity plus a couple of at-a-glance facts about the super admin account
/// viewing it — everything this page shows, in one call.
#[derive(serde::Serialize)]
pub struct SocietyProfilePageData {
    pub profile: Option<SocietyProfile>,
    pub super_admin_email: String,
    pub account_created_at: DateTime<Utc>,
}

/// Society identity is sensitive letterhead/registration data, deliberately restricted
/// to the super_admin designation itself rather than any designation holding
/// `accounting.view` — a treasurer or accountant role has no business editing what the
/// society's legal name and registration number are.
pub async fn get_society_profile(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<SocietyProfilePageData>, ApiError> {
    if !user.is_admin() {
        return Err(forbidden(
            "Only the super admin can view the society profile",
        ));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let profile = accounting::db::get_society_profile(&mut conn)
        .await
        .map_err(internal_err)?;
    let account_created_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT created_at FROM users WHERE user_id = $1")
            .bind(user.user_id)
            .fetch_one(&mut *conn)
            .await
            .map_err(internal_err)?;
    Ok(Json(SocietyProfilePageData {
        profile,
        super_admin_email: user.email.clone(),
        account_created_at,
    }))
}

#[derive(serde::Deserialize)]
pub struct SetSocietyProfileRequest {
    pub name: String,
    pub registration_number: Option<String>,
    pub address: String,
    pub state: Option<String>,
    pub phone: Option<String>,
}

pub async fn set_society_profile(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SetSocietyProfileRequest>,
) -> Result<StatusCode, ApiError> {
    if !user.is_admin() {
        return Err(forbidden(
            "Only the super admin can edit the society profile",
        ));
    }
    if let Some(s) = &payload.state {
        if !accounting::geography::INDIAN_STATES_AND_UTS.contains(&s.as_str()) {
            return Err(bad_request(
                "state must be one of India's states or union territories",
            ));
        }
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    accounting::db::set_society_profile(
        &mut conn,
        accounting::db::SetSocietyProfile {
            name: &payload.name,
            registration_number: payload.registration_number.as_deref(),
            address: &payload.address,
            state: payload.state.as_deref(),
            phone: payload.phone.as_deref(),
            updated_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_payment_instructions(
    SessionUser(_user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Option<PaymentInstructions>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let instructions = accounting::db::get_payment_instructions(&mut conn)
        .await
        .map_err(internal_err)?;
    Ok(Json(instructions))
}

#[derive(serde::Deserialize)]
pub struct SetPaymentInstructionsRequest {
    pub account_holder_name: String,
    pub bank_name: String,
    pub account_number: String,
    pub ifsc_code: String,
    pub branch: Option<String>,
    pub contact_email: Option<String>,
}

pub async fn set_payment_instructions(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SetPaymentInstructionsRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    accounting::db::set_payment_instructions(
        &mut conn,
        accounting::db::SetPaymentInstructions {
            account_holder_name: &payload.account_holder_name,
            bank_name: &payload.bank_name,
            account_number: &payload.account_number,
            ifsc_code: &payload.ifsc_code,
            branch: payload.branch.as_deref(),
            contact_email: payload.contact_email.as_deref(),
            updated_by: user.user_id,
        },
    )
    .await
    .map_err(internal_err)?;
    Ok(StatusCode::NO_CONTENT)
}
