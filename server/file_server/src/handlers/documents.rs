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
pub struct UserCreateDocumentPayload {
    pub title: String,
    pub description: Option<String>,
}

/// POST /api/v1/documents/upload
/// Allows residents to upload a document linked to their flat (10MB limit).
pub async fn user_create_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<(StatusCode, Json<DocumentWithMetadata>), (StatusCode, Json<String>)> {
    let mut file_data = None;
    let mut file_content_type = None;
    let mut file_name = None;
    let mut payload: Option<UserCreateDocumentPayload> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(format!("Multipart error: {}", e)),
        )
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            file_content_type = field.content_type().map(|c| c.to_string());
            file_name = field.file_name().map(|f| f.to_string());
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Failed to read file: {}", e)),
                )
            })?;
            file_data = Some(data);
        } else if name == "payload" {
            let text = field.text().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Failed to read payload: {}", e)),
                )
            })?;
            let p: UserCreateDocumentPayload = serde_json::from_str(&text).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Invalid JSON payload: {}", e)),
                )
            })?;
            payload = Some(p);
        }
    }

    let payload = payload.ok_or((
        StatusCode::BAD_REQUEST,
        Json("Missing payload field in multipart request".to_string()),
    ))?;

    let file_bytes = file_data.ok_or((
        StatusCode::BAD_REQUEST,
        Json("Missing file field in multipart request".to_string()),
    ))?;

    if file_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Uploaded file cannot be empty".to_string()),
        ));
    }

    // 10MB max size validation for users
    let max_size = 10 * 1024 * 1024;
    if file_bytes.len() > max_size {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json("File size exceeds the 10MB limit".to_string()),
        ));
    }

    // File type validation (PDF, Word, Excel, Text)
    if let Some(name) = file_name {
        let ext = name.split('.').next_back().unwrap_or("").to_lowercase();
        if !VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(
                    "Invalid file type. Only PDF, Word, Excel, and Text files are allowed."
                        .to_string(),
                ),
            ));
        }
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("File name is required to determine file type.".to_string()),
        ));
    }

    let mut result = state
        .document_manager
        .user_create_document_atomic(&user, &payload.title, payload.description.as_deref())
        .await
        .map_err(|e| {
            if e == "User does not have an active flat ownership" {
                (StatusCode::FORBIDDEN, Json(e))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(e))
            }
        })?;

    // Derive S3 Key: {public/private}/{first_char_of_uuid_lowercase}/{document_uuid}
    let key = crate::document::derive_s3_key(
        result.document.process_id,
        DocumentAccessLevel::Private,
        result.document.document_id,
    );
    let content_type = file_content_type.unwrap_or_else(|| "application/octet-stream".to_string());

    // Upload to S3
    if let Err(err) = state
        .document_manager
        .s3_accessor
        .upload_file(
            &key,
            aws_sdk_s3::primitives::ByteStream::from(file_bytes),
            &content_type,
        )
        .await
    {
        // Rollback/delete the created document from the DB on failure
        let _ = state
            .document_manager
            .delete_document_permanently(result.document.document_id)
            .await;

        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to upload document file to S3: {}", err)),
        ));
    }

    // Generate presigned URL for the response
    if let Ok(url) = state
        .document_manager
        .generate_presigned_url(&result.document)
        .await
    {
        result.download_url = Some(url);
    }

    Ok((StatusCode::CREATED, Json(result)))
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

/// POST /api/v1/admin/documents/create
/// Creates a new document with optional flat mapping and metadata — atomically.
pub async fn admin_create_document(
    _session_user: SessionUser,
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<(StatusCode, Json<DocumentWithMetadata>), (StatusCode, Json<String>)> {
    let mut file_data = None;
    let mut file_content_type = None;
    let mut file_name = None;
    let mut payload: Option<AdminCreateDocumentPayload> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(format!("Multipart error: {}", e)),
        )
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let content_type = field.content_type().map(|c| c.to_string());
            file_name = field.file_name().map(|f| f.to_string());
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Failed to read file: {}", e)),
                )
            })?;
            file_data = Some(data);
            file_content_type = content_type;
        } else if name == "payload" {
            let text = field.text().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Failed to read payload: {}", e)),
                )
            })?;
            let p: AdminCreateDocumentPayload = serde_json::from_str(&text).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Invalid JSON payload: {}", e)),
                )
            })?;
            payload = Some(p);
        }
    }

    let payload = payload.ok_or((
        StatusCode::BAD_REQUEST,
        Json("Missing payload field in multipart request".to_string()),
    ))?;

    let file_bytes = file_data.ok_or((
        StatusCode::BAD_REQUEST,
        Json("Missing file field in multipart request".to_string()),
    ))?;

    if file_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Uploaded file cannot be empty".to_string()),
        ));
    }

    // 1GB max size validation
    let max_size = 1024 * 1024 * 1024;
    if file_bytes.len() > max_size {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json("File size exceeds the 1GB limit".to_string()),
        ));
    }

    // File type validation (PDF, Word, Excel, Text)
    if let Some(name) = file_name {
        let ext = name.split('.').next_back().unwrap_or("").to_lowercase();
        if !VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(
                    "Invalid file type. Only PDF, Word, Excel, and Text files are allowed."
                        .to_string(),
                ),
            ));
        }
    } else {
        // Reject if no file name is provided to be safe
        return Err((
            StatusCode::BAD_REQUEST,
            Json("File name is required to determine file type.".to_string()),
        ));
    }

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

    let mut result = state
        .document_manager
        .admin_create_document_atomic(
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

    // Derive S3 Key: {public/private}/{first_char_of_uuid_lowercase}/{document_uuid}
    let key = crate::document::derive_s3_key(
        result.document.process_id,
        access_level,
        result.document.document_id,
    );
    let content_type = file_content_type.unwrap_or_else(|| "application/octet-stream".to_string());

    // Upload to S3
    if let Err(err) = state
        .document_manager
        .s3_accessor
        .upload_file(
            &key,
            aws_sdk_s3::primitives::ByteStream::from(file_bytes),
            &content_type,
        )
        .await
    {
        // Rollback/delete the created document from the DB on failure
        let _ = state
            .document_manager
            .delete_document_permanently(result.document.document_id)
            .await;

        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to upload document file to S3: {}", err)),
        ));
    }

    // Generate presigned URL for the response
    if let Ok(url) = state
        .document_manager
        .generate_presigned_url(&result.document)
        .await
    {
        result.download_url = Some(url);
    }

    Ok((StatusCode::CREATED, Json(result)))
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
