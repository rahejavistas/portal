use crate::document::{
    DocumentAccessLevel, DocumentCategory, DocumentType, DocumentWithMetadata, MetadataInput,
};
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base::flat::FlatIdentifier;
use uuid::Uuid;

pub const VALID_UPLOAD_EXTENSIONS: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "txt"];

// ==========================================
// REGULAR RESIDENT / USER ENDPOINTS
// ==========================================

/// POST /api/v1/documents
/// Lists all active (non-deleted) documents that the currently authenticated resident is authorized to view.
pub async fn list_documents(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<DocumentWithMetadata>>, (StatusCode, Json<String>)> {
    match state.document_manager.list_visible_documents(&user).await {
        Ok(docs) => Ok(Json(docs)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to retrieve documents: {}", e)),
        )),
    }
}

/// POST /api/v1/documents/:id
/// Fetches details of a specific active document, enforcing access control permissions.
pub async fn get_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
) -> Result<Json<DocumentWithMetadata>, (StatusCode, Json<String>)> {
    match state
        .document_manager
        .get_document(document_id, &user)
        .await
    {
        Ok(Some(doc)) => Ok(Json(doc)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json("Document not found".to_string()),
        )),
        Err(e) => {
            if e == "FORBIDDEN" {
                Err((
                    StatusCode::FORBIDDEN,
                    Json(
                        "Access Denied: You do not have permissions to view this document"
                            .to_string(),
                    ),
                ))
            } else {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(format!("Database error: {}", e)),
                ))
            }
        }
    }
}

#[derive(serde::Deserialize)]
pub struct UserUploadUrlRequest {
    pub title: String,
    pub filename: String,
    /// Client-provided content_type is ignored for security; derived from filename extension.
    pub content_type: Option<String>,
}

#[derive(serde::Serialize)]
pub struct UserUploadUrlResponse {
    pub document_id: String,
    pub upload_url: String,
}

/// POST /api/v1/documents/upload-url
/// Returns a presigned PUT URL for direct client-to-S3 upload.
/// Creates an unconfirmed document record in the DB first.
pub async fn user_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<UserUploadUrlRequest>,
) -> Result<(StatusCode, Json<UserUploadUrlResponse>), (StatusCode, Json<String>)> {
    // Sanitize title
    let title = payload.title.trim().to_string();
    if title.is_empty() || title.len() > 200 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Title must be between 1 and 200 characters".to_string()),
        ));
    }

    // Validate file extension
    let ext = payload
        .filename
        .split('.')
        .next_back()
        .unwrap_or("")
        .to_lowercase();
    if !VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(format!(
                "Invalid file type '{}'. Allowed: {:?}",
                ext, VALID_UPLOAD_EXTENSIONS
            )),
        ));
    }

    // SECURITY: Always derive content-type from validated extension
    let content_type = match ext.as_str() {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
    .to_string();

    let (result, upload_url, _key) = state
        .document_manager
        .create_upload_intent(&user, &title, &content_type)
        .await
        .map_err(|e| {
            if e == "User does not have an active flat ownership" {
                (StatusCode::FORBIDDEN, Json(e))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(e))
            }
        })?;

    Ok((
        StatusCode::OK,
        Json(UserUploadUrlResponse {
            document_id: result.document.document_id.to_string(),
            upload_url,
        }),
    ))
}

#[derive(serde::Deserialize)]
pub struct UserConfirmUploadRequest {
    pub document_id: Uuid,
}

#[derive(serde::Serialize)]
pub struct UserConfirmUploadResponse {
    pub status: String,
}

/// POST /api/v1/documents/confirm-upload
/// Confirms that the client successfully uploaded the file to S3.
pub async fn user_confirm_upload(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<UserConfirmUploadRequest>,
) -> Result<Json<UserConfirmUploadResponse>, (StatusCode, Json<String>)> {
    // Verify the document exists and belongs to this user
    let _doc = state
        .document_manager
        .get_document(payload.document_id, &user)
        .await
        .map_err(|e| {
            if e == "FORBIDDEN" {
                (StatusCode::FORBIDDEN, Json("Access denied".to_string()))
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

    // Confirm the upload (HEAD check + DB update)
    state
        .document_manager
        .confirm_upload(payload.document_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    Ok(Json(UserConfirmUploadResponse {
        status: "confirmed".to_string(),
    }))
}

// ==========================================
// ADMINISTRATIVE CRUD ENDPOINTS
// ==========================================

#[derive(serde::Deserialize)]
pub struct CreateMetadataPayload {
    pub subcategory: Option<String>,
    pub tags: Vec<String>,
    pub document_date: chrono::NaiveDate,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_until: Option<chrono::NaiveDate>,
}

#[derive(serde::Deserialize)]
pub struct AdminCreateDocumentPayload {
    pub title: String,
    pub description: Option<String>,
    pub category: DocumentCategory,
    pub document_type: DocumentType,
    /// Optional access level override. Falls back to category default if not provided.
    pub access_level: Option<DocumentAccessLevel>,
    pub metadata: Option<CreateMetadataPayload>,
    /// Flat mapping for RESIDENT_SPECIFIC documents (wing + flat_number).
    pub flat: Option<FlatPayload>,
}

#[derive(serde::Deserialize)]
pub struct AdminUpdateDocumentPayload {
    pub title: String,
    pub description: Option<String>,
    pub category: DocumentCategory,
    pub document_type: DocumentType,
    /// Optional access level override. Falls back to category default if not provided.
    pub access_level: Option<DocumentAccessLevel>,
    pub metadata: Option<CreateMetadataPayload>,
    /// Flat mapping for RESIDENT_SPECIFIC documents (wing + flat_number).
    pub flat: Option<FlatPayload>,
}

/// Raw flat input from API consumers. Validated and normalized into `FlatIdentifier`.
#[derive(serde::Deserialize)]
pub struct FlatPayload {
    pub wing: String,
    pub flat_number: String,
}

impl FlatPayload {
    /// Validates and converts raw input into a normalized `FlatIdentifier`.
    pub fn into_identifier(self) -> Result<FlatIdentifier, String> {
        FlatIdentifier::new(&self.wing, &self.flat_number)
    }
}

/// Validates that the document_type is valid for the given category.
/// Returns a user-friendly error string if the combination is invalid.
fn validate_category_type(
    category: DocumentCategory,
    document_type: DocumentType,
) -> Result<(), String> {
    if !category.is_valid_type(document_type) {
        return Err(format!(
            "Document type '{:?}' is not valid for category '{:?}'. Valid types: {:?}",
            document_type,
            category,
            category.valid_types()
        ));
    }
    Ok(())
}

/// POST /api/v1/admin/documents/list
/// Lists ALL documents in the database (including soft-deleted ones) with zero visibility filters.
pub async fn admin_list_documents(
    _session_user: SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<DocumentWithMetadata>>, (StatusCode, Json<String>)> {
    match state.document_manager.admin_list_all_documents().await {
        Ok(docs) => Ok(Json(docs)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Database error: {}", e)),
        )),
    }
}

/// POST /api/v1/admin/documents/:id/get
/// Fetches details of any document (including soft-deleted ones) without visibility filters.
pub async fn admin_get_document(
    _session_user: SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
) -> Result<Json<DocumentWithMetadata>, (StatusCode, Json<String>)> {
    match state
        .document_manager
        .admin_get_any_document(document_id)
        .await
    {
        Ok(Some(doc)) => Ok(Json(doc)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json("Document not found".to_string()),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Database error: {}", e)),
        )),
    }
}

#[derive(serde::Deserialize)]
pub struct AdminUploadUrlRequest {
    pub title: String,
    pub description: Option<String>,
    pub category: DocumentCategory,
    pub document_type: DocumentType,
    /// Optional access level override. Falls back to category default if not provided.
    pub access_level: Option<DocumentAccessLevel>,
    pub metadata: Option<CreateMetadataPayload>,
    /// Flat mapping for RESIDENT_SPECIFIC documents (wing + flat_number).
    pub flat: Option<FlatPayload>,
    pub filename: String,
    pub content_type: Option<String>,
}

#[derive(serde::Serialize)]
pub struct AdminUploadUrlResponse {
    pub document_id: String,
    pub upload_url: String,
}

#[derive(serde::Deserialize)]
pub struct AdminConfirmUploadRequest {
    pub document_id: Uuid,
}

#[derive(serde::Serialize)]
pub struct AdminConfirmUploadResponse {
    pub status: String,
}

/// POST /api/v1/admin/documents/upload-url
/// Creates an unconfirmed document with optional flat mapping and metadata — atomically,
/// and returns a presigned PUT URL for direct client-to-S3 upload.
pub async fn admin_upload_url(
    _session_user: SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminUploadUrlRequest>,
) -> Result<(StatusCode, Json<AdminUploadUrlResponse>), (StatusCode, Json<String>)> {
    // Validate file extension
    let ext = payload
        .filename
        .split('.')
        .next_back()
        .unwrap_or("")
        .to_lowercase();
    if !VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(format!(
                "Invalid file type '{}'. Allowed: {:?}",
                ext, VALID_UPLOAD_EXTENSIONS
            )),
        ));
    }

    // SECURITY: Derive content-type from the validated file extension
    let content_type = match ext.as_str() {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
    .to_string();

    // Validate category ↔ type combination before hitting DB
    validate_category_type(payload.category, payload.document_type)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    // Use provided access level, or fall back to the category default
    let access_level = payload
        .access_level
        .unwrap_or_else(|| payload.category.default_access_level());

    // Validate: RESIDENT_SPECIFIC documents MUST have a flat mapping
    if payload.category == DocumentCategory::ResidentSpecific && payload.flat.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                "RESIDENT_SPECIFIC documents require a flat mapping (wing + flat_number)."
                    .to_string(),
            ),
        ));
    }

    // Validate and normalize the flat identifier
    let flat_id = match payload.flat {
        Some(fp) => Some(
            fp.into_identifier()
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?,
        ),
        None => None,
    };

    let metadata = payload.metadata.as_ref().map(|m| MetadataInput {
        subcategory: m.subcategory.as_deref(),
        tags: m.tags.as_slice(),
        document_date: m.document_date,
        valid_from: m.valid_from,
        valid_until: m.valid_until,
    });

    let (result, upload_url, _key) = state
        .document_manager
        .admin_create_upload_intent(
            &payload.title,
            payload.description.as_deref(),
            payload.category,
            payload.document_type,
            access_level,
            flat_id.as_ref(),
            metadata,
            &content_type,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(AdminUploadUrlResponse {
            document_id: result.document.document_id.to_string(),
            upload_url,
        }),
    ))
}

/// POST /api/v1/admin/documents/confirm-upload
/// Confirms that the client successfully uploaded the file to S3.
pub async fn admin_confirm_upload(
    _session_user: SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminConfirmUploadRequest>,
) -> Result<Json<AdminConfirmUploadResponse>, (StatusCode, Json<String>)> {
    // Admin confirm upload just delegates to the document manager to do HEAD check.
    // Ensure the document exists before confirming.
    state
        .document_manager
        .confirm_upload(payload.document_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    Ok(Json(AdminConfirmUploadResponse {
        status: "confirmed".to_string(),
    }))
}

/// POST /api/v1/admin/documents/:id/update
/// Updates document core properties, metadata, and flat assignment — atomically.
pub async fn admin_update_document(
    _session_user: SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
    Json(payload): Json<AdminUpdateDocumentPayload>,
) -> Result<Json<DocumentWithMetadata>, (StatusCode, Json<String>)> {
    // Validate category ↔ type combination before hitting DB
    validate_category_type(payload.category, payload.document_type)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    // Use provided access level, or fall back to the category default
    let access_level = payload
        .access_level
        .unwrap_or_else(|| payload.category.default_access_level());

    // Validate: RESIDENT_SPECIFIC documents MUST have a flat mapping
    if payload.category == DocumentCategory::ResidentSpecific && payload.flat.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                "RESIDENT_SPECIFIC documents require a flat mapping (wing + flat_number)."
                    .to_string(),
            ),
        ));
    }

    // Validate and normalize the flat identifier
    let flat_id = match payload.flat {
        Some(fp) => Some(
            fp.into_identifier()
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?,
        ),
        None => None,
    };

    let metadata = payload.metadata.as_ref().map(|m| MetadataInput {
        subcategory: m.subcategory.as_deref(),
        tags: m.tags.as_slice(),
        document_date: m.document_date,
        valid_from: m.valid_from,
        valid_until: m.valid_until,
    });

    let result = state
        .document_manager
        .admin_update_document_atomic(
            document_id,
            &payload.title,
            payload.description.as_deref(),
            payload.category,
            payload.document_type,
            access_level,
            flat_id.as_ref(),
            metadata,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    Ok(Json(result))
}

/// POST /api/v1/admin/documents/:id/delete
/// Toggles soft delete (is_deleted = TRUE) for a specific document.
pub async fn admin_delete_document(
    _session_user: SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<String>)> {
    state
        .document_manager
        .admin_soft_delete_document(document_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    Ok(StatusCode::NO_CONTENT)
}
