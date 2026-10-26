//! Owner-managed tenant mapping for a flat — see `process::StepOwner::Tenant`.

use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, forbidden, too_many_requests, ApiError, ResultExt};
use crate::state::AppState;
use axum::{extract::State, Json};
use rand::{thread_rng, Rng};

async fn owned_flat_id(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
    ownership_id: i32,
) -> Result<i32, ApiError> {
    process::db::get_active_ownerships_for_user(conn, user_id)
        .await
        .internal_err()?
        .into_iter()
        .find(|&(oid, _)| oid == ownership_id)
        .map(|(_, flat_id)| flat_id)
        .ok_or_else(|| forbidden("Access Denied: you do not own this flat"))
}

/// Whether `email` belongs to a user who already owns a flat or holds a designation —
/// mapping such an account as a tenant would silently cross-link a real resident's or
/// admin's identity with zero consent, even though they'd inherit nothing from it.
async fn email_belongs_to_a_privileged_account(
    conn: &mut sqlx::PgConnection,
    email: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS ( \
            SELECT 1 FROM users u WHERE u.email = $1 AND ( \
                EXISTS (SELECT 1 FROM flat_ownerships fo WHERE fo.user_id = u.user_id AND fo.is_active = TRUE) \
                OR EXISTS (SELECT 1 FROM user_designations ud WHERE ud.user_id = u.user_id AND ud.is_active = TRUE) \
            ) \
         )",
    )
    .bind(email)
    .fetch_one(conn)
    .await
}

/// Same "set your password" link as `forgot_password`, framed as a tenant invite.
async fn send_tenant_invite(state: &AppState, email: &str) {
    let token: String = thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    if state
        .permissions_manager
        .create_reset_token(email, &token, state.forgot_password_expiry.as_secs() as i64)
        .await
        .is_err()
    {
        return;
    }
    let set_password_link = format!(
        "{}/reset-password?token={}",
        email::config::portal_base_url(),
        token
    );
    let payload = email::EmailMessage {
        to: email.to_string(),
        subject: "Raheja Vistas: You've been added as a tenant".to_string(),
        heading: "Set Up Your Portal Account".to_string(),
        paragraphs: vec![
            "You've been added as a tenant on the Raheja Vistas Portal.".to_string(),
            "Click below to set your password and sign in.".to_string(),
        ],
        code: None,
        cta: Some(email::Cta {
            label: "Set Password".to_string(),
            url: set_password_link,
        }),
        attachment: None,
    };
    let _ = email::send_email(&payload).await;
}

#[derive(serde::Deserialize)]
pub struct AddTenantRequest {
    pub ownership_id: i32,
    pub tenant_email: String,
    pub expires_at: chrono::NaiveDate,
    pub rental_agreement_document_id: uuid::Uuid,
}

#[derive(serde::Serialize)]
pub struct AddTenantResponse {
    pub tenant_ownership_id: i32,
    pub tenant_user_id: i32,
}

/// POST /api/v1/tenants/add
/// Deactivates any existing active tenant for this ownership first — at most one
/// active tenant per flat.
pub async fn add_tenant(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AddTenantRequest>,
) -> Result<Json<AddTenantResponse>, ApiError> {
    let email = payload.tenant_email.trim().to_lowercase();
    if email.is_empty() || email.len() > 254 || !email.contains('@') || !email.contains('.') {
        return Err(bad_request("Invalid email format."));
    }
    if email == user.email.trim().to_lowercase() {
        return Err(bad_request("You cannot add yourself as a tenant."));
    }

    let mut tx = state.pg_pool.begin().await.internal_err()?;
    let flat_id = owned_flat_id(&mut tx, user.user_id, payload.ownership_id).await?;

    if email_belongs_to_a_privileged_account(&mut tx, &email)
        .await
        .internal_err()?
    {
        return Err(bad_request(
            "This email belongs to an existing resident or admin account and can't be mapped as a tenant.",
        ));
    }

    // Checked only once the caller is confirmed to actually own this flat — cap
    // per-recipient so this can't be used to bomb or phish an arbitrary inbox.
    if state
        .rate_limiters
        .email_invite_limiter
        .check_key(&email)
        .is_err()
    {
        return Err(too_many_requests(
            "Too many invites sent to this email recently. Please try again later.",
        ));
    }

    let belongs = state
        .document_manager
        .document_belongs_to_flat(payload.rental_agreement_document_id, flat_id)
        .await
        .internal_err()?;
    if !belongs {
        return Err(bad_request(
            "Rental agreement document is not attached to this flat",
        ));
    }

    sqlx::query(
        "UPDATE tenant_ownerships SET is_active = FALSE, ended_at = NOW() \
         WHERE ownership_id = $1 AND is_active = TRUE",
    )
    .bind(payload.ownership_id)
    .execute(&mut *tx)
    .await
    .internal_err()?;

    let tenant = state
        .permissions_manager
        .create_tenant_user(&email, Some(user.user_id))
        .await
        .map_err(bad_request)?;

    let (tenant_ownership_id,): (i32,) = sqlx::query_as(
        "INSERT INTO tenant_ownerships \
            (ownership_id, tenant_user_id, expires_at, rental_agreement_document_id, created_by) \
         VALUES ($1, $2, $3, $4, $5) RETURNING tenant_ownership_id",
    )
    .bind(payload.ownership_id)
    .bind(tenant.user_id)
    .bind(payload.expires_at)
    .bind(payload.rental_agreement_document_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await
    .internal_err()?;

    tx.commit().await.internal_err()?;

    send_tenant_invite(&state, &email).await;

    Ok(Json(AddTenantResponse {
        tenant_ownership_id,
        tenant_user_id: tenant.user_id,
    }))
}

#[derive(serde::Deserialize)]
pub struct RemoveTenantRequest {
    pub ownership_id: i32,
}

#[derive(serde::Serialize)]
pub struct RemoveTenantResponse {
    pub status: String,
}

/// POST /api/v1/tenants/remove
pub async fn remove_tenant(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<RemoveTenantRequest>,
) -> Result<Json<RemoveTenantResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    owned_flat_id(&mut conn, user.user_id, payload.ownership_id).await?;

    sqlx::query(
        "UPDATE tenant_ownerships SET is_active = FALSE, ended_at = NOW() \
         WHERE ownership_id = $1 AND is_active = TRUE",
    )
    .bind(payload.ownership_id)
    .execute(&mut *conn)
    .await
    .internal_err()?;

    Ok(Json(RemoveTenantResponse {
        status: "removed".to_string(),
    }))
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct CurrentTenant {
    pub tenant_ownership_id: i32,
    pub tenant_email: String,
    pub expires_at: chrono::NaiveDate,
}

#[derive(serde::Deserialize)]
pub struct GetTenantRequest {
    pub ownership_id: i32,
}

/// POST /api/v1/tenants/current — the flat's current active tenant, if any.
pub async fn get_tenant(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<GetTenantRequest>,
) -> Result<Json<Option<CurrentTenant>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    owned_flat_id(&mut conn, user.user_id, payload.ownership_id).await?;

    let tenant: Option<CurrentTenant> = sqlx::query_as(
        "SELECT t.tenant_ownership_id, u.email AS tenant_email, t.expires_at \
         FROM tenant_ownerships t \
         JOIN users u ON u.user_id = t.tenant_user_id \
         JOIN flat_ownerships fo ON fo.ownership_id = t.ownership_id \
         WHERE t.ownership_id = $1 AND t.is_active = TRUE \
           AND t.expires_at >= CURRENT_DATE AND fo.is_active = TRUE",
    )
    .bind(payload.ownership_id)
    .fetch_optional(&mut *conn)
    .await
    .internal_err()?;

    Ok(Json(tenant))
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct MyFlat {
    pub ownership_id: i32,
    pub wing: String,
    pub flat_number: String,
}

/// POST /api/v1/tenants/my-flats — the caller's own active flat ownerships.
pub async fn my_flats(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<MyFlat>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    let flats: Vec<MyFlat> = sqlx::query_as(
        "SELECT fo.ownership_id, f.wing, f.flat_number FROM flat_ownerships fo \
         JOIN flats f ON f.flat_id = fo.flat_id \
         WHERE fo.user_id = $1 AND fo.is_active = TRUE",
    )
    .bind(user.user_id)
    .fetch_all(&mut *conn)
    .await
    .internal_err()?;
    Ok(Json(flats))
}
