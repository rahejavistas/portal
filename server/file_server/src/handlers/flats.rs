use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use axum::{extract::State, Json};
use base::flat::FlatIdentifier;
use rust_decimal::Decimal;

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct FlatSummary {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub area_sqft: Option<Decimal>,
    pub is_fire_flat: bool,
    pub owner_email: Option<String>,
}

pub async fn list_flats(
    State(state): State<AppState>,
    SessionUser(_caller): SessionUser,
) -> Result<Json<Vec<FlatSummary>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let flats: Vec<FlatSummary> = sqlx::query_as(
        "SELECT f.flat_id, f.wing, f.flat_number, f.area_sqft, f.is_fire_flat, u.email AS owner_email \
         FROM flats f \
         LEFT JOIN flat_ownerships fo ON fo.flat_id = f.flat_id AND fo.is_active = TRUE \
         LEFT JOIN users u ON u.user_id = fo.user_id \
         ORDER BY f.wing, f.flat_number",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(internal_err)?;

    Ok(Json(flats))
}

#[derive(serde::Deserialize)]
pub struct CreateFlatRequest {
    pub wing: String,
    pub flat_number: String,
    pub area_sqft: Option<Decimal>,
}

pub async fn create_flat(
    State(state): State<AppState>,
    SessionUser(_caller): SessionUser,
    Json(payload): Json<CreateFlatRequest>,
) -> Result<Json<FlatSummary>, ApiError> {
    let flat = FlatIdentifier::new(&payload.wing, &payload.flat_number).map_err(bad_request)?;
    if payload.area_sqft.is_some_and(|a| a <= Decimal::ZERO) {
        return Err(bad_request("area_sqft must be positive"));
    }
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;

    let (flat_id, wing, flat_number, area_sqft, is_fire_flat): (
        i32,
        String,
        String,
        Option<Decimal>,
        bool,
    ) = sqlx::query_as(
        "INSERT INTO flats (wing, flat_number, area_sqft) VALUES ($1, $2, $3) \
         ON CONFLICT (wing, flat_number) DO UPDATE SET wing = EXCLUDED.wing \
         RETURNING flat_id, wing, flat_number, area_sqft, is_fire_flat",
    )
    .bind(flat.wing())
    .bind(flat.flat_number())
    .bind(payload.area_sqft)
    .fetch_one(&mut *conn)
    .await
    .map_err(internal_err)?;

    Ok(Json(FlatSummary {
        flat_id,
        wing,
        flat_number,
        area_sqft,
        is_fire_flat,
        owner_email: None,
    }))
}

#[derive(serde::Deserialize)]
pub struct BulkCreateFlatsRequest {
    pub wing: String,
    pub flat_numbers: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct BulkCreateFlatsResponse {
    pub created: usize,
}

pub async fn bulk_create_flats(
    State(state): State<AppState>,
    SessionUser(_caller): SessionUser,
    Json(payload): Json<BulkCreateFlatsRequest>,
) -> Result<Json<BulkCreateFlatsResponse>, ApiError> {
    if payload.flat_numbers.is_empty() {
        return Err(bad_request("flat_numbers must not be empty"));
    }

    let mut normalized = Vec::with_capacity(payload.flat_numbers.len());
    for raw in &payload.flat_numbers {
        let flat = FlatIdentifier::new(&payload.wing, raw).map_err(bad_request)?;
        normalized.push(flat.flat_number().to_string());
    }
    let wing = FlatIdentifier::new(&payload.wing, &normalized[0])
        .map_err(bad_request)?
        .wing()
        .to_string();

    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let rows = sqlx::query(
        "INSERT INTO flats (wing, flat_number) \
         SELECT $1, v.flat_number FROM UNNEST($2::text[]) AS v(flat_number) \
         ON CONFLICT (wing, flat_number) DO NOTHING",
    )
    .bind(&wing)
    .bind(&normalized)
    .execute(&mut *conn)
    .await
    .map_err(internal_err)?;

    Ok(Json(BulkCreateFlatsResponse {
        created: rows.rows_affected() as usize,
    }))
}

#[derive(serde::Deserialize)]
pub struct SetFlatFireFlagRequest {
    pub flat_id: i32,
    pub is_fire_flat: bool,
}

pub async fn set_flat_fire_flag(
    State(state): State<AppState>,
    SessionUser(_caller): SessionUser,
    Json(payload): Json<SetFlatFireFlagRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let updated = sqlx::query("UPDATE flats SET is_fire_flat = $1 WHERE flat_id = $2")
        .bind(payload.is_fire_flat)
        .bind(payload.flat_id)
        .execute(&mut *conn)
        .await
        .map_err(internal_err)?;
    if updated.rows_affected() == 0 {
        return Err(bad_request("Unknown flat_id"));
    }
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(serde::Deserialize)]
pub struct DeleteFlatRequest {
    pub flat_id: i32,
}

pub async fn delete_flat(
    State(state): State<AppState>,
    SessionUser(_caller): SessionUser,
    Json(payload): Json<DeleteFlatRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;

    let has_active_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM flat_ownerships WHERE flat_id = $1 AND is_active = TRUE)",
    )
    .bind(payload.flat_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(internal_err)?;
    if has_active_owner {
        return Err(bad_request("This flat is occupied."));
    }

    let result = sqlx::query("DELETE FROM flats WHERE flat_id = $1")
        .bind(payload.flat_id)
        .execute(&mut *conn)
        .await;

    match result {
        Ok(r) if r.rows_affected() == 0 => Err(bad_request("Unknown flat_id")),
        Ok(_) => Ok(Json(serde_json::json!({ "status": "ok" }))),
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23503") => Err(bad_request(
            "Cannot remove this flat - it has billing, document, or process records attached.",
        )),
        Err(e) => Err(internal_err(e)),
    }
}
