//! Optional attachment on an internal reviewer note. Reviewer-only and kept out of the
//! documents library.

use super::{
    build_actor_context, can_view_all_processes, load_authorized_process, ApiError,
    DocumentUrlResponse, ResultExt,
};
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use process::db::CommentAttachmentId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_FILE_NAME_CHARS: usize = 200;

fn bad_request(message: &str) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(message.to_string()))
}

pub(super) fn attachment_key(process_id: Uuid, attachment_id: CommentAttachmentId) -> String {
    format!(
        "process-comment-attachments/{}/{}",
        process_id, attachment_id
    )
}

/// 403 unless the caller may review the step the note is about.
pub(super) fn require_may_review_step(
    template: &process::ProcessTemplate,
    actor: &process::state_machine::ActorContext,
    template_step_id: i32,
) -> Result<(), ApiError> {
    let step_owner = process::state_machine::build_step_info_map(template)
        .get(&template_step_id)
        .map(|info| info.filled_by.clone())
        .ok_or_else(|| bad_request("Step not found"))?;
    if actor.may_review(&step_owner) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: you do not act on this step".to_string()),
        ))
    }
}

/// 400 unless the attachment's file has actually reached storage.
pub(super) async fn require_uploaded(
    state: &AppState,
    process_id: Uuid,
    attachment_id: CommentAttachmentId,
) -> Result<(), ApiError> {
    let size = state
        .document_manager
        .s3_accessor()
        .head_object(&attachment_key(process_id, attachment_id))
        .await
        .internal_err()?;
    match size {
        Some(bytes) if bytes > 0 => Ok(()),
        _ => Err(bad_request("The attachment file was not uploaded")),
    }
}

#[derive(Deserialize)]
pub struct NoteAttachmentUploadUrlRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub filename: String,
}

#[derive(Serialize)]
pub struct NoteAttachmentUploadUrlResponse {
    pub attachment_id: CommentAttachmentId,
    pub upload_url: String,
    pub content_type: String,
}

/// POST /api/v1/admin/process/add-note/attachment-upload-url
/// The attachment is bound to this process, step and caller; only they can attach it.
pub async fn admin_note_attachment_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<NoteAttachmentUploadUrlRequest>,
) -> Result<Json<NoteAttachmentUploadUrlResponse>, ApiError> {
    let file_name = payload
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if file_name.is_empty() || file_name.chars().count() > MAX_FILE_NAME_CHARS {
        return Err(bad_request(
            "File name must be between 1 and 200 characters",
        ));
    }
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    if !crate::handlers::documents::VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err(bad_request(&format!(
            "Invalid file type '{}'. Allowed: {:?}",
            ext,
            crate::handlers::documents::VALID_UPLOAD_EXTENSIONS
        )));
    }
    let content_type = crate::handlers::documents::derive_content_type_from_ext(&ext);

    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    let actor = build_actor_context(&mut conn, &state.permissions_manager, &user).await?;
    let (_info, template) = load_authorized_process(
        &mut conn,
        &user,
        payload.process_id,
        actor.is_admin,
        &actor.designation_codes,
    )
    .await?;
    require_may_review_step(&template, &actor, payload.template_step_id)?;

    let attachment_id = process::db::create_comment_attachment(
        &mut conn,
        payload.process_id,
        payload.template_step_id,
        user.user_id,
        file_name,
        &content_type,
    )
    .await
    .internal_err()?;
    drop(conn);

    let upload_url = state
        .document_manager
        .s3_accessor()
        .create_presigned_put_url(
            &attachment_key(payload.process_id, attachment_id),
            &content_type,
        )
        .await
        .internal_err()?;

    Ok(Json(NoteAttachmentUploadUrlResponse {
        attachment_id,
        upload_url,
        content_type,
    }))
}

#[derive(Deserialize)]
pub struct NoteAttachmentDownloadRequest {
    pub process_id: Uuid,
    pub attachment_id: CommentAttachmentId,
}

/// POST /api/v1/admin/process/note-attachment/download-url
/// Same audience as the internal notes themselves.
pub async fn admin_note_attachment_download_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<NoteAttachmentDownloadRequest>,
) -> Result<Json<DocumentUrlResponse>, ApiError> {
    if !can_view_all_processes(&user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Not authorized to view internal notes".to_string()),
        ));
    }

    let mut conn = state.pg_pool.acquire().await.internal_err()?;
    load_authorized_process(
        &mut conn,
        &user,
        payload.process_id,
        true,
        &std::collections::HashSet::new(),
    )
    .await?;
    let attached = process::db::comment_attachment_is_attached(
        &mut conn,
        payload.process_id,
        payload.attachment_id,
    )
    .await
    .internal_err()?;
    drop(conn);
    if !attached {
        return Err((
            StatusCode::NOT_FOUND,
            Json("Attachment not found".to_string()),
        ));
    }

    let download_url = state
        .document_manager
        .s3_accessor()
        .create_presigned_url(&attachment_key(payload.process_id, payload.attachment_id))
        .await
        .internal_err()?;
    Ok(Json(DocumentUrlResponse { download_url }))
}
