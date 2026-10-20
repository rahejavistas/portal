//! Document upload/download URLs for a process.

use super::{build_actor_context, load_authorized_process, ApiError, ResultExt};
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
/// Creates an upload intent for an admin-uploaded process document. The backend
/// derives the flat and forces RESIDENT_SPECIFIC.
pub async fn process_admin_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessAdminUploadUrlRequest>,
) -> Result<(StatusCode, Json<ProcessAdminUploadUrlResponse>), ApiError> {
    let title = payload.title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Title must be between 1 and 200 characters".to_string()),
        ));
    }

    let mut conn = state.pg_pool.acquire().await.internal_err()?;

    let actor = build_actor_context(&mut conn, &state.permissions_manager, &user).await?;
    let (info, template) = load_authorized_process(
        &mut conn,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;
    super::require_reviewer_standing(&template, &actor)?;

    let Some(oid) = info.ownership_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("This process has no associated flat to attach the document to".to_string()),
        ));
    };
    let Some(flat_id) = get_flat_info_for_ownership(&mut conn, oid)
        .await
        .internal_err()?
        .map(|f| f.flat_id)
    else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("This process has no associated flat to attach the document to".to_string()),
        ));
    };

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
                title,
                description: None,
                category: crate::document::RESIDENT_SPECIFIC_CATEGORY,
                metadata: None,
                content_type: &content_type,
                uploaded_by: Some(user.user_id),
            },
            Some(flat_id),
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

#[derive(Deserialize)]
pub struct ProcessAdminConfirmUploadRequest {
    pub process_id: Uuid,
    pub document_id: Uuid,
}

#[derive(Serialize)]
pub struct ProcessAdminConfirmUploadResponse {
    pub status: String,
}

/// POST /api/v1/admin/process/confirm-upload
/// Separate from `handlers::documents::admin_confirm_upload` — that one needs
/// document-category permissions a plain reviewer won't have.
pub async fn process_admin_confirm_upload(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessAdminConfirmUploadRequest>,
) -> Result<Json<ProcessAdminConfirmUploadResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    let actor = build_actor_context(&mut conn, &state.permissions_manager, &user).await?;
    let (info, template) = load_authorized_process(
        &mut conn,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;
    super::require_reviewer_standing(&template, &actor)?;

    let flat_id = match info.ownership_id {
        Some(oid) => get_flat_info_for_ownership(&mut conn, oid)
            .await
            .internal_err()?
            .map(|f| f.flat_id),
        None => None,
    };
    let belongs = match flat_id {
        Some(fid) => state
            .document_manager
            .document_belongs_to_flat(payload.document_id, fid)
            .await
            .internal_err()?,
        None => false,
    };
    if !belongs {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Document is not attached to this process".to_string()),
        ));
    }

    state
        .document_manager
        .confirm_upload(payload.document_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    Ok(Json(ProcessAdminConfirmUploadResponse {
        status: "confirmed".to_string(),
    }))
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
    let can_view_all = super::can_view_all_processes(&user);
    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    let mut actor = build_actor_context(&mut conn, &state.permissions_manager, &user).await?;

    // Auth check: is the caller allowed to view this process at all?
    let owner_id = {
        let (info, template) = load_authorized_process(
            &mut conn,
            &user,
            payload.process_id,
            can_view_all,
            &actor.designation_codes,
        )
        .await?;
        super::resolve_flat_roles(
            &mut conn,
            &mut actor,
            &user,
            info.ownership_id,
            info.user_id,
        )
        .await?;

        // Non-owners (reviewers, tenants) may only fetch documents actually attached to
        // this process; a tenant-only caller only those on `Tenant` stages.
        if info.user_id != user.user_id {
            let tenant_only = !can_view_all && actor.sees_only_tenant_stages();
            let tenant_field_ids: std::collections::HashSet<i32> =
                process::state_machine::build_step_info_map(&template)
                    .values()
                    .filter(|i| matches!(i.filled_by, process::StepOwner::Tenant))
                    .flat_map(|i| i.fields.iter().map(|f| f.field_id))
                    .collect();
            let history = get_field_history(&mut conn, payload.process_id)
                .await
                .internal_err()?;
            let attached = history.iter().any(|row| {
                row.document_id == Some(payload.document_id)
                    && (!tenant_only || tenant_field_ids.contains(&row.template_field_id))
            });
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
