//! Admin CRUD for the signer directory (`document_signers`).

use crate::document::SignerDirectoryEntry;
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};

type ApiError = (StatusCode, Json<String>);

fn internal_err(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string()))
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}

#[derive(serde::Deserialize, Default)]
pub struct ListSignersPayload {
    /// The Signer Directory oversight card sends `true` to see everyone (for
    /// activate/deactivate). The "Add Signers" picker sends no body at all, which
    /// defaults to `false` — it has no business offering a deactivated person.
    #[serde(default)]
    pub include_inactive: bool,
}

/// POST /api/v1/admin/signers/list
/// Requires `process.reviewer` (the same permission that attaches signers in the
/// first place — there's no separate signatures-management permission). Response
/// deliberately excludes phone/email — see `SignerDirectoryEntry`.
pub async fn list(
    State(state): State<AppState>,
    _session_user: SessionUser,
    body: axum::body::Bytes,
) -> Result<Json<Vec<SignerDirectoryEntry>>, ApiError> {
    let payload: ListSignersPayload = serde_json::from_slice(&body).unwrap_or_default();
    let signers = if payload.include_inactive {
        state.document_manager.list_signers().await
    } else {
        state.document_manager.list_active_signers().await
    }
    .map_err(internal_err)?;
    Ok(Json(signers))
}

#[derive(serde::Deserialize)]
pub struct SetActivePayload {
    pub signer_id: i32,
    pub is_active: bool,
}

/// POST /api/v1/admin/signers/set-active
pub async fn set_active(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(payload): Json<SetActivePayload>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .document_manager
        .set_signer_active(payload.signer_id, payload.is_active, user.user_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}
