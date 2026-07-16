pub mod manager;

pub use manager::{AdminUploadIntentInput, DocumentManager};

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "document_category", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentCategory {
    Public,
    ResidentSpecific,
    Committee,
    Finance,
    Governance,
    Operational,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Document {
    pub document_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub category: DocumentCategory,
    pub uploaded_date: DateTime<Utc>,
    pub is_deleted: bool,
    pub is_user_uploaded: bool,
    pub is_upload_confirmed: bool,
    pub uploaded_by: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct GenericDocumentMetadata {
    pub document_id: Uuid,
    pub subcategory: Option<String>,
    pub tags: Vec<String>,
    pub document_date: NaiveDate,
    pub valid_from: Option<NaiveDate>,
    pub valid_until: Option<NaiveDate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentFlatInfo {
    pub wing: String,
    pub flat_number: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentWithMetadata {
    pub document: Document,
    pub metadata: Option<GenericDocumentMetadata>,
    #[serde(default)]
    pub flat: Option<DocumentFlatInfo>,
    #[serde(default)]
    pub download_url: Option<String>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DocumentWithMetadata {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let document = Document::from_row(row)?;

        // When using LEFT JOIN on generic_document_metadata, a non-matching row
        // leaves document_date as NULL. Since the column is NOT NULL in the schema,
        // a NULL value reliably indicates that no metadata row exists.
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
            (Ok(wing), Ok(flat_number)) => Some(DocumentFlatInfo { wing, flat_number }),
            _ => None,
        };

        Ok(Self {
            document,
            metadata,
            flat,
            download_url: None,
        })
    }
}

pub fn derive_s3_key(category: DocumentCategory, document_id: Uuid) -> String {
    let prefix = match category {
        DocumentCategory::ResidentSpecific => "private",
        _ => "public",
    };
    let uuid_str = document_id.to_string();
    let first_char = uuid_str.chars().next().unwrap_or('a').to_ascii_lowercase();
    format!("{}/{}/{}", prefix, first_char, uuid_str)
}

/// Structured input for document metadata fields, used by the manager's
/// atomic create/update methods. Replaces raw tuples to satisfy clippy::type_complexity.
pub struct MetadataInput<'a> {
    pub subcategory: Option<&'a str>,
    pub tags: &'a [String],
    pub document_date: NaiveDate,
    pub valid_from: Option<NaiveDate>,
    pub valid_until: Option<NaiveDate>,
}
