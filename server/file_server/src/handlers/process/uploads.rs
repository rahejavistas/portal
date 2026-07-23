//! Document upload/download URLs for a process.

use super::{load_authorized_process, ApiError, ResultExt};
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::db::{get_field_history, get_flat_info_for_ownership};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct DocumentUrlRequest {
    pub process_id: Uuid,
    pub document_id: Uuid,
}

#[derive(Serialize)]
pub struct DocumentUrlResponse {
    pub download_url: String,
}

#[derive(Deserialize)]
pub struct ProcessAdminUploadUrlRequest {
    pub process_id: Uuid,
    pub title: String,
    pub filename: String,
    pub content_type: Option<String>,
}

#[derive(Serialize)]
pub struct ProcessAdminUploadUrlResponse {
    pub document_id: String,
    pub upload_url: String,
}

/// POST /api/v1/admin/process/upload-url
/// Creates an upload intent for a process document uploaded by an admin.
/// The backend securely derives the flat and forces RESIDENT_SPECIFIC (private) semantics.
pub async fn process_admin_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessAdminUploadUrlRequest>,
) -> Result<(StatusCode, Json<ProcessAdminUploadUrlResponse>), ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;

    let (info, _) = load_authorized_process(&mut conn, &user, payload.process_id, true).await?;

    let mut flat_id: Option<i32> = None;
    if let Some(oid) = info.ownership_id {
        flat_id = get_flat_info_for_ownership(&mut conn, oid)
            .await
            .unwrap_or(None)
            .map(|f| f.flat_id);
    }

    // Validate file extension
    let ext = payload
        .filename
        .split('.')
        .next_back()
        .unwrap_or("")
        .to_lowercase();
    if !crate::handlers::documents::VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(format!(
                "Invalid file type '{}'. Allowed: {:?}",
                ext,
                crate::handlers::documents::VALID_UPLOAD_EXTENSIONS
            )),
        ));
    }

    let content_type = crate::handlers::documents::derive_content_type_from_ext(ext.as_str());

    let (result, upload_url, _key) = state
        .document_manager
        .admin_create_upload_intent_by_id(
            crate::document::AdminUploadIntentInput {
                title: &payload.title,
                description: None,
                category: crate::document::RESIDENT_SPECIFIC_CATEGORY,
                metadata: None,
                content_type: &content_type,
                uploaded_by: Some(user.user_id),
            },
            flat_id,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(ProcessAdminUploadUrlResponse {
            document_id: result.document.document_id.to_string(),
            upload_url,
        }),
    ))
}

/// POST /api/v1/process/download-url
/// On-demand presigned download URL for a process document.
pub async fn process_download_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<DocumentUrlRequest>,
) -> Result<Json<DocumentUrlResponse>, ApiError> {
    // Read-only oversight (process.view) can download documents attached to any process,
    // same as a reviewer — this handler never writes anything.
    let can_view_all = user.has_permission(authentication::Permission::ProcessReviewer)
        || user.has_permission(authentication::Permission::ProcessView);

    // Auth check: is the caller allowed to view this process at all?
    let owner_id = {
        let mut conn = state.pg_pool.acquire().await.internal_err()?;
        let (info, _) =
            load_authorized_process(&mut conn, &user, payload.process_id, can_view_all).await?;

        // Non-owners (reviewers) may only fetch documents actually attached to this process.
        if info.user_id != user.user_id {
            let history = get_field_history(&mut conn, payload.process_id)
                .await
                .internal_err()?;
            let attached = history
                .iter()
                .any(|row| row.document_id == Some(payload.document_id));
            if !attached {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json("Document is not attached to this process".to_string()),
                ));
            }
        }

        info.user_id
    };

    let owner = state
        .permissions_manager
        .get_user_by_id(owner_id)
        .await
        .internal_err()?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json("Process owner not found".to_string()),
            )
        })?;

    let doc = state
        .document_manager
        .get_document(payload.document_id, &owner)
        .await
        .map_err(|e| {
            if e == "FORBIDDEN" {
                (StatusCode::FORBIDDEN, Json(e))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(e))
            }
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json("Document not found".to_string()),
            )
        })?;

    let url = doc.download_url.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json("Failed to generate download URL".to_string()),
        )
    })?;

    Ok(Json(DocumentUrlResponse { download_url: url }))
}
