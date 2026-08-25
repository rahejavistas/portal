mod category;
pub mod manager;
pub mod signatures;
pub mod signers;
mod subcategory;

pub use manager::{AdminUploadIntentInput, DocumentManager, UpdateDocumentInput};
pub use signatures::{SignatureRequirement, SignatureStatus};
pub use signers::{Signer, SignerDirectoryEntry};

use base::flat::FlatIdentifier;
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

/// The well-known code of the two protected system categories (seeded, never deletable).
/// Every other category is admin-defined at runtime — see `document_categories` table.
pub const PUBLIC_CATEGORY: &str = "PUBLIC";
pub const RESIDENT_SPECIFIC_CATEGORY: &str = "RESIDENT_SPECIFIC";

/// A row of `document_categories`. PUBLIC and RESIDENT_SPECIFIC are protected system
/// rows; every other category's access is governed by document_category_designations.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct DocumentCategoryDef {
    pub code: String,
    pub label: String,
    pub is_public_read: bool,
    pub is_flat_scoped: bool,
    pub is_system: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Document {
    pub document_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub uploaded_date: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_deleted: bool,
    pub is_user_uploaded: bool,
    pub is_upload_confirmed: bool,
    pub uploaded_by: Option<i32>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct GenericDocumentMetadata {
    pub document_id: Uuid,
    /// FK into `document_subcategories`. UI-only tag, not access control.
    pub subcategory_id: Option<i32>,
    pub subcategory_code: Option<String>,
    pub subcategory_label: Option<String>,
    pub tags: Vec<String>,
    pub document_date: NaiveDate,
    pub valid_from: Option<NaiveDate>,
    pub valid_until: Option<NaiveDate>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct DocumentSubcategoryDef {
    pub subcategory_id: i32,
    pub category: String,
    pub code: String,
    pub label: String,
}

/// A category plus the subcategories nested under it, so the frontend gets both
/// in a single fetch instead of separately polling a subcategories endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryWithSubcategories {
    #[serde(flatten)]
    pub category: DocumentCategoryDef,
    pub subcategories: Vec<DocumentSubcategoryDef>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentFlatInfo {
    #[serde(flatten)]
    pub flat: FlatIdentifier,
}

/// One document a bulk action couldn't apply to, and why (missing, or not manageable
/// by the caller's permissions).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BulkActionSkip {
    pub document_id: Uuid,
    pub reason: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BulkActionResult {
    pub succeeded: Vec<Uuid>,
    pub skipped: Vec<BulkActionSkip>,
}

/// A single row of the `document_category_designations` access matrix: whether a
/// designation can read and/or manage (upload, sign, delete) a document category.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct CategoryDesignation {
    pub category: String,
    pub designation_code: String,
    pub can_read: bool,
    pub can_manage: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentWithMetadata {
    pub document: Document,
    pub metadata: Option<GenericDocumentMetadata>,
    #[serde(default)]
    pub flat: Option<DocumentFlatInfo>,
    #[serde(default)]
    pub owner_email: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DocumentWithMetadata {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let document = Document::from_row(row)?;

        // document_date is NOT NULL in the schema, so NULL here means no metadata row.
        let has_metadata = row.try_get::<NaiveDate, _>("document_date").is_ok();

        let metadata = if has_metadata {
            Some(GenericDocumentMetadata::from_row(row)?)
        } else {
            None
        };

        // wing/flat_number are only present when the query joins document_flats/flats
        // (e.g. the admin document list); absent otherwise.
        let flat = match (
            row.try_get::<String, _>("wing"),
            row.try_get::<String, _>("flat_number"),
        ) {
            (Ok(wing), Ok(flat_number)) => Some(DocumentFlatInfo {
                flat: FlatIdentifier::from_normalized(wing, flat_number),
            }),
            _ => None,
        };

        let owner_email = row.try_get::<String, _>("owner_email").ok();

        Ok(Self {
            document,
            metadata,
            flat,
            owner_email,
            download_url: None,
        })
    }
}

pub fn derive_s3_key(document_id: Uuid) -> String {
    let uuid_str = document_id.to_string();
    let first_char = uuid_str.chars().next().unwrap_or('a').to_ascii_lowercase();
    format!("documents/{}/{}", first_char, uuid_str)
}

/// Structured input for document metadata fields, used by the manager's
/// atomic create/update methods. Replaces raw tuples to satisfy clippy::type_complexity.
pub struct MetadataInput<'a> {
    pub subcategory_id: Option<i32>,
    pub tags: &'a [String],
    pub document_date: NaiveDate,
    pub valid_from: Option<NaiveDate>,
    pub valid_until: Option<NaiveDate>,
}
