pub mod manager;

pub use manager::DocumentManager;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "document_category", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentCategory {
    ResidentSpecific,
    ProcessSpecific,
    Generic,
    MeetingMinutes,
    Operational,
}

impl DocumentCategory {
    /// Returns the enforced default access level for each document category.
    /// RESIDENT_SPECIFIC documents are always PRIVATE; all other categories default to PUBLIC.
    pub fn default_access_level(&self) -> DocumentAccessLevel {
        match self {
            DocumentCategory::ResidentSpecific => DocumentAccessLevel::Private,
            DocumentCategory::ProcessSpecific => DocumentAccessLevel::Private,
            DocumentCategory::Generic => DocumentAccessLevel::Public,
            DocumentCategory::MeetingMinutes => DocumentAccessLevel::Public,
            DocumentCategory::Operational => DocumentAccessLevel::Public,
        }
    }

    /// Returns the list of valid `DocumentType` variants for this category.
    /// Used for server-side validation before hitting the DB constraint.
    pub fn valid_types(&self) -> &[DocumentType] {
        match self {
            DocumentCategory::ResidentSpecific => &[
                DocumentType::ResidentProperty,
                DocumentType::ResidentKyc,
                DocumentType::ResidentRental,
                DocumentType::ResidentGeneral,
            ],
            DocumentCategory::ProcessSpecific => &[DocumentType::ProcessGeneral],
            DocumentCategory::Generic => {
                &[DocumentType::GenericRulebook, DocumentType::GenericBylaws]
            }
            DocumentCategory::MeetingMinutes => &[
                DocumentType::MinutesAgm,
                DocumentType::MinutesGbm,
                DocumentType::MinutesMgmt,
            ],
            DocumentCategory::Operational => &[
                DocumentType::OperationalVendorContract,
                DocumentType::OperationalPayment,
                DocumentType::OperationalSla,
            ],
        }
    }

    /// Checks if a `DocumentType` is valid for this category.
    pub fn is_valid_type(&self, doc_type: DocumentType) -> bool {
        self.valid_types().contains(&doc_type)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(
    type_name = "document_access_level",
    rename_all = "SCREAMING_SNAKE_CASE"
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentAccessLevel {
    Public,
    CommitteeOnly,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "document_type", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentType {
    ResidentProperty,
    ResidentKyc,
    ResidentRental,
    ResidentGeneral,
    ProcessGeneral,
    GenericRulebook,
    GenericBylaws,
    MinutesAgm,
    MinutesGbm,
    MinutesMgmt,
    OperationalVendorContract,
    OperationalPayment,
    OperationalSla,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Document {
    pub document_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub category: DocumentCategory,
    pub document_type: DocumentType,
    pub access_level: DocumentAccessLevel,
    pub restricted_committee_roles: Vec<authentication::UserRole>,
    pub uploaded_date: DateTime<Utc>,
    pub is_deleted: bool,
    pub is_user_uploaded: bool,
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
pub struct DocumentWithMetadata {
    pub document: Document,
    pub metadata: Option<GenericDocumentMetadata>,
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

        Ok(Self {
            document,
            metadata,
            download_url: None,
        })
    }
}

pub fn derive_s3_key(access_level: DocumentAccessLevel, document_id: Uuid) -> String {
    let prefix = match access_level {
        DocumentAccessLevel::Public | DocumentAccessLevel::CommitteeOnly => "public",
        DocumentAccessLevel::Private => "private",
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
