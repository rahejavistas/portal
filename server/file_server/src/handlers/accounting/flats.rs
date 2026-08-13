use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use axum::{extract::State, Json};
use base::flat::FlatIdentifier;

/// Resolves a flat's numeric id from wing + flat_number — every accounting endpoint
/// takes `flat_id`, but nowhere else in this app surfaces that number; admins only
/// ever type wing/flat_number, same as everywhere else in the portal.
#[derive(serde::Deserialize)]
pub struct ResolveFlatRequest {
    pub wing: String,
    pub flat_number: String,
}

#[derive(serde::Serialize)]
pub struct ResolveFlatResponse {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
}

pub async fn resolve_flat(
    State(state): State<AppState>,
    Json(payload): Json<ResolveFlatRequest>,
) -> Result<Json<ResolveFlatResponse>, ApiError> {
    let flat = FlatIdentifier::new(&payload.wing, &payload.flat_number).map_err(bad_request)?;
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let flat_id = accounting::db::resolve_flat_id(&mut conn, flat.wing(), flat.flat_number())
        .await
        .map_err(internal_err)?
        .ok_or_else(|| {
            bad_request(format!(
                "No flat {}-{} found",
                flat.wing(),
                flat.flat_number()
            ))
        })?;

    Ok(Json(ResolveFlatResponse {
        flat_id,
        wing: flat.wing().to_string(),
        flat_number: flat.flat_number().to_string(),
    }))
}
