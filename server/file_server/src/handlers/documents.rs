use crate::document::{DocumentWithMetadata, MetadataInput, RESIDENT_SPECIFIC_CATEGORY};
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base::flat::FlatIdentifier;
use uuid::Uuid;

pub const VALID_UPLOAD_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "txt", "jpg", "jpeg", "png",
];

type ApiError = (StatusCode, Json<String>);

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

pub fn derive_content_type_from_ext(ext: &str) -> String {
    match ext {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "txt" => "text/plain",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn internal_err(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string()))
}

fn forbidden(msg: impl Into<String>) -> ApiError {
    (StatusCode::FORBIDDEN, Json(msg.into()))
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}

fn not_found(msg: &str) -> ApiError {
    (StatusCode::NOT_FOUND, Json(msg.to_string()))
}

/// Content type is always derived server-side from the extension, never trusted from the client.
fn validate_upload_extension(filename: &str) -> Result<String, ApiError> {
    let ext = filename.split('.').next_back().unwrap_or("").to_lowercase();
    if !VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err(bad_request(format!(
            "Invalid file type '{}'. Allowed: {:?}",
            ext, VALID_UPLOAD_EXTENSIONS
        )));
    }
    Ok(derive_content_type_from_ext(&ext))
}

/// Resolves a `CreateMetadataPayload`'s `subcategory_code` (if any) against `category` and
/// builds the `MetadataInput` the manager layer expects. Shared by create and update.
async fn resolve_metadata<'a>(
    state: &AppState,
    category: &str,
    metadata: &'a Option<CreateMetadataPayload>,
) -> Result<Option<MetadataInput<'a>>, ApiError> {
    match metadata.as_ref() {
        Some(m) => {
            let subcategory_id = match &m.subcategory_code {
                Some(code) => Some(
                    state
                        .document_manager
                        .resolve_subcategory_id(category, code)
                        .await
                        .map_err(bad_request)?,
                ),
                None => None,
            };
            Ok(Some(MetadataInput {
                subcategory_id,
                tags: m.tags.as_slice(),
                document_date: m.document_date,
                valid_from: m.valid_from,
                valid_until: m.valid_until,
            }))
        }
        None => Ok(None),
    }
}

type FlatAndMetadata<'a> = (Option<FlatIdentifier>, Option<MetadataInput<'a>>);

async fn validate_flat_and_metadata<'a>(
    state: &AppState,
    category: &str,
    flat: Option<FlatPayload>,
    metadata: &'a Option<CreateMetadataPayload>,
) -> Result<FlatAndMetadata<'a>, ApiError> {
    if category == RESIDENT_SPECIFIC_CATEGORY && flat.is_none() {
        return Err(bad_request(
            "RESIDENT_SPECIFIC documents require a flat mapping (wing + flat_number).",
        ));
    }

    let flat_id = flat
        .map(FlatPayload::into_identifier)
        .transpose()
        .map_err(bad_request)?;

    let metadata = resolve_metadata(state, category, metadata).await?;

    Ok((flat_id, metadata))
}

/// Errors with 403 unless `user` can manage `category`.
async fn require_manageable_category(
    state: &AppState,
    user: &authentication::User,
    category: &str,
) -> Result<(), ApiError> {
    let manageable = state
        .document_manager
        .manageable_categories(user)
        .await
        .map_err(internal_err)?;
    match manageable {
        None => Ok(()),
        Some(categories) if categories.contains(category) => Ok(()),
        Some(_) => Err(forbidden(format!(
            "Access denied: you cannot manage the '{}' category",
            category
        ))),
    }
}

/// Fetches `document_id` (404 if missing, ignoring soft-delete) and errors with 403
/// unless `user` can manage its category. Shared by every admin document handler that
/// needs "does this document exist, and can this caller touch it" in one step.
async fn require_manageable_document(
    state: &AppState,
    user: &authentication::User,
    document_id: Uuid,
) -> Result<DocumentWithMetadata, ApiError> {
    let doc = state
        .document_manager
        .admin_get_any_document(document_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Document not found"))?;

    require_manageable_category(state, user, &doc.document.category).await?;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Resident endpoints
// ---------------------------------------------------------------------------

pub async fn list_documents(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<DocumentWithMetadata>>, ApiError> {
    let docs = state
        .document_manager
        .list_visible_documents(&user)
        .await
        .map_err(internal_err)?;
    Ok(Json(docs))
}

pub async fn get_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
) -> Result<Json<DocumentWithMetadata>, ApiError> {
    let doc = state
        .document_manager
        .get_document(document_id, &user)
        .await
        .map_err(|e| {
            if e == "FORBIDDEN" {
                forbidden("Access Denied: You do not have permissions to view this document")
            } else {
                internal_err(e)
            }
        })?;
    doc.map(Json).ok_or_else(|| not_found("Document not found"))
}

#[derive(serde::Deserialize)]
pub struct UserUploadUrlRequest {
    pub title: String,
    pub filename: String,
    pub content_type: Option<String>,
}

#[derive(serde::Serialize)]
pub struct UserUploadUrlResponse {
    pub document_id: String,
    pub upload_url: String,
}

/// Returns a presigned S3 upload URL, creating an unconfirmed document record first.
pub async fn user_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<UserUploadUrlRequest>,
) -> Result<(StatusCode, Json<UserUploadUrlResponse>), ApiError> {
    let title = payload.title.trim().to_string();
    if title.is_empty() || title.len() > 200 {
        return Err(bad_request("Title must be between 1 and 200 characters"));
    }

    let content_type = validate_upload_extension(&payload.filename)?;

    let (result, upload_url, _key) = state
        .document_manager
        .create_upload_intent(&user, &title, &content_type)
        .await
        .map_err(|e| {
            if e == "User does not have an active flat ownership" {
                forbidden(e)
            } else {
                internal_err(e)
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

pub async fn user_confirm_upload(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<UserConfirmUploadRequest>,
) -> Result<Json<UserConfirmUploadResponse>, ApiError> {
    state
        .document_manager
        .get_document(payload.document_id, &user)
        .await
        .map_err(|e| {
            if e == "FORBIDDEN" {
                forbidden("Access denied")
            } else {
                internal_err(e)
            }
        })?
        .ok_or_else(|| not_found("Document not found"))?;

    state
        .document_manager
        .confirm_upload(payload.document_id)
        .await
        .map_err(bad_request)?;

    Ok(Json(UserConfirmUploadResponse {
        status: "confirmed".to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Admin endpoints
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct CreateMetadataPayload {
    pub subcategory_code: Option<String>,
    pub tags: Vec<String>,
    pub document_date: chrono::NaiveDate,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_until: Option<chrono::NaiveDate>,
}

#[derive(serde::Deserialize)]
pub struct FlatPayload {
    pub wing: String,
    pub flat_number: String,
}

impl FlatPayload {
    pub fn into_identifier(self) -> Result<FlatIdentifier, String> {
        FlatIdentifier::new(&self.wing, &self.flat_number)
    }
}

/// `documents.all.manage` holders see everything; others see only categories they
/// hold a `can_manage` grant for.
pub async fn admin_list_documents(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<DocumentWithMetadata>>, ApiError> {
    let manageable = state
        .document_manager
        .manageable_categories(&user)
        .await
        .map_err(internal_err)?;

    let all_docs = state
        .document_manager
        .admin_list_all_documents()
        .await
        .map_err(internal_err)?;

    let visible = match manageable {
        None => all_docs,
        Some(categories) => all_docs
            .into_iter()
            .filter(|doc| categories.contains(&doc.document.category))
            .collect(),
    };
    Ok(Json(visible))
}

pub async fn admin_list_manageable_categories(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::document::CategoryWithSubcategories>>, ApiError> {
    let manageable = state
        .document_manager
        .manageable_categories(&user)
        .await
        .map_err(internal_err)?;

    let all_categories = state
        .document_manager
        .list_document_categories()
        .await
        .map_err(internal_err)?;

    let visible = match manageable {
        None => all_categories,
        Some(categories) => all_categories
            .into_iter()
            .filter(|c| categories.contains(&c.category.code))
            .collect(),
    };
    Ok(Json(visible))
}

pub async fn admin_get_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
) -> Result<Json<DocumentWithMetadata>, ApiError> {
    require_manageable_document(&state, &user, document_id)
        .await
        .map(Json)
}

#[derive(serde::Deserialize)]
pub struct AdminUploadUrlRequest {
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub metadata: Option<CreateMetadataPayload>,
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

pub async fn admin_upload_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminUploadUrlRequest>,
) -> Result<(StatusCode, Json<AdminUploadUrlResponse>), ApiError> {
    let content_type = validate_upload_extension(&payload.filename)?;

    require_manageable_category(&state, &user, &payload.category).await?;
    let (flat_id, metadata) =
        validate_flat_and_metadata(&state, &payload.category, payload.flat, &payload.metadata)
            .await?;

    let (result, upload_url, _key) = state
        .document_manager
        .admin_create_upload_intent(
            crate::document::AdminUploadIntentInput {
                title: &payload.title,
                description: payload.description.as_deref(),
                category: &payload.category,
                metadata,
                content_type: &content_type,
                uploaded_by: Some(user.user_id),
            },
            flat_id.as_ref(),
        )
        .await
        .map_err(internal_err)?;

    Ok((
        StatusCode::CREATED,
        Json(AdminUploadUrlResponse {
            document_id: result.document.document_id.to_string(),
            upload_url,
        }),
    ))
}

pub async fn admin_confirm_upload(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminConfirmUploadRequest>,
) -> Result<Json<AdminConfirmUploadResponse>, ApiError> {
    require_manageable_document(&state, &user, payload.document_id).await?;

    state
        .document_manager
        .confirm_upload(payload.document_id)
        .await
        .map_err(bad_request)?;

    Ok(Json(AdminConfirmUploadResponse {
        status: "confirmed".to_string(),
    }))
}

#[derive(serde::Deserialize)]
pub struct UpdateDocumentPayload {
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub metadata: Option<CreateMetadataPayload>,
}

/// Updates title/description/category/metadata for an existing document. Changing category
/// can change who's able to see the document (categories are what access is keyed on), so
/// callers should surface that clearly — this handler just enforces it isn't done blindly:
/// the caller must be able to manage both the document's current category and the target
/// one, and category changes into/out of RESIDENT_SPECIFIC are rejected outright since that
/// category's visibility also depends on flat mapping, which this endpoint doesn't touch.
pub async fn admin_update_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
    Json(payload): Json<UpdateDocumentPayload>,
) -> Result<Json<DocumentWithMetadata>, ApiError> {
    let existing = require_manageable_document(&state, &user, document_id).await?;
    let current_category = existing.document.category;

    let title = payload.title.trim().to_string();
    if title.is_empty() || title.len() > 200 {
        return Err(bad_request("Title must be between 1 and 200 characters"));
    }

    if payload.category != current_category
        && (payload.category == RESIDENT_SPECIFIC_CATEGORY
            || current_category == RESIDENT_SPECIFIC_CATEGORY)
    {
        return Err(bad_request(
            "Changing a document into or out of RESIDENT_SPECIFIC isn't supported here — \
             that category's visibility also depends on its flat mapping.",
        ));
    }

    if payload.category != current_category {
        require_manageable_category(&state, &user, &payload.category).await?;
    }

    let metadata = resolve_metadata(&state, &payload.category, &payload.metadata).await?;

    let updated = state
        .document_manager
        .update_document(
            document_id,
            crate::document::UpdateDocumentInput {
                title: &title,
                description: payload.description.as_deref(),
                category: &payload.category,
                metadata,
            },
        )
        .await
        .map_err(bad_request)?;

    Ok(Json(updated))
}

pub async fn admin_delete_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Path(document_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_manageable_document(&state, &user, document_id).await?;

    state
        .document_manager
        .admin_soft_delete_document(document_id)
        .await
        .map_err(internal_err)?;

    Ok(StatusCode::NO_CONTENT)
}
