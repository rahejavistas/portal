use crate::document::{
    Document, DocumentAccessLevel, DocumentCategory, DocumentType, DocumentWithMetadata,
    GenericDocumentMetadata, MetadataInput,
};
use authentication::UserRole;
use base::flat::FlatIdentifier;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct DocumentManager {
    pub pg_pool: PgPool,
    pub s3_accessor: base::s3::S3Accessor,
}

impl DocumentManager {
    pub fn new(pg_pool: PgPool, s3_accessor: base::s3::S3Accessor) -> Self {
        Self {
            pg_pool,
            s3_accessor,
        }
    }

    pub async fn generate_presigned_url(&self, doc: &Document) -> Result<String, String> {
        let key = super::derive_s3_key(doc.process_id, doc.access_level, doc.document_id);
        self.s3_accessor
            .create_presigned_url(&key)
            .await
            .map_err(|e| e.to_string())
    }

    /// Fetches all allowed document IDs for a user based on access levels, roles, and direct resident mapping.
    pub async fn get_allowed_document_ids(
        &self,
        user: &authentication::User,
    ) -> Result<std::collections::HashSet<Uuid>, String> {
        let mut allowed_ids = std::collections::HashSet::new();

        let is_admin = user.has_role(UserRole::Admin);
        let is_committee = user.is_committee_or_admin();

        if is_admin {
            // Admin bypasses all checks and sees all non-deleted files
            let admin_ids: Vec<Uuid> =
                sqlx::query_scalar("SELECT document_id FROM documents WHERE NOT is_deleted")
                    .fetch_all(&self.pg_pool)
                    .await
                    .map_err(|e| e.to_string())?;
            return Ok(admin_ids.into_iter().collect());
        }

        // 1. Fetch all PUBLIC documents
        let public_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT document_id FROM documents WHERE access_level = 'PUBLIC' AND NOT is_deleted",
        )
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        allowed_ids.extend(public_ids);

        // 2. Fetch COMMITTEE_ONLY documents if the user is a committee member
        if is_committee {
            let committee_ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT document_id FROM documents \
                 WHERE access_level = 'COMMITTEE_ONLY' \
                   AND NOT is_deleted \
                   AND ( \
                       cardinality(restricted_committee_roles) = 0 \
                       OR restricted_committee_roles && $1 \
                   )",
            )
            .bind(&user.roles)
            .fetch_all(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;
            allowed_ids.extend(committee_ids);
        }

        // 3. Fetch PRIVATE documents assigned to flat(s) owned/occupied by the user
        let private_assigned_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT df.document_id \
             FROM document_flats df \
             JOIN flat_ownerships fo ON df.flat_id = fo.flat_id \
             WHERE fo.user_id = $1 AND fo.is_active = TRUE",
        )
        .bind(user.user_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        allowed_ids.extend(private_assigned_ids);

        // 4. Fetch PRIVATE documents associated with processes owned by the user
        let process_assigned_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT d.document_id \
             FROM documents d \
             JOIN processes p ON d.process_id = p.process_id \
             WHERE p.user_id = $1 AND NOT d.is_deleted",
        )
        .bind(user.user_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        allowed_ids.extend(process_assigned_ids);

        Ok(allowed_ids)
    }

    /// Fetches a non-deleted document by its ID and validates if the user has authorization to view it.
    /// Returns `Err("FORBIDDEN")` if access control evaluation fails.
    pub async fn get_document(
        &self,
        document_id: Uuid,
        user: &authentication::User,
    ) -> Result<Option<DocumentWithMetadata>, String> {
        // A. Check if the document exists (excluding soft-deleted)
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT document_id FROM documents WHERE document_id = $1 AND NOT is_deleted",
        )
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        if exists.is_none() {
            return Ok(None);
        }

        // B. Get precomputed allowed document IDs for this user
        let allowed_ids = self.get_allowed_document_ids(user).await?;
        if !allowed_ids.contains(&document_id) {
            return Err("FORBIDDEN".to_string());
        }

        // C. Fetch the authorized document
        let mut doc: Option<DocumentWithMetadata> = sqlx::query_as(
            "SELECT \
                d.document_id, d.title, d.description, d.category, d.document_type, d.access_level, d.restricted_committee_roles, d.uploaded_date, d.is_deleted, d.is_user_uploaded, d.process_id, \
                g.subcategory, g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             WHERE d.document_id = $1 AND NOT d.is_deleted",
        )
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        if let Some(ref mut d) = doc {
            let url = self.generate_presigned_url(&d.document).await?;
            d.download_url = Some(url);
        }

        Ok(doc)
    }

    /// Lists all non-deleted documents the user is authorized to view.
    /// Pushes the allowed IDs into SQL to avoid fetching all documents and filtering in Rust.
    pub async fn list_visible_documents(
        &self,
        user: &authentication::User,
    ) -> Result<Vec<DocumentWithMetadata>, String> {
        let allowed_ids = self.get_allowed_document_ids(user).await?;
        let ids: Vec<Uuid> = allowed_ids.into_iter().collect();

        let docs: Vec<DocumentWithMetadata> = sqlx::query_as(
            "SELECT \
                d.document_id, d.title, d.description, d.category, d.document_type, d.access_level, d.restricted_committee_roles, d.uploaded_date, d.is_deleted, d.is_user_uploaded, d.process_id, \
                g.subcategory, g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             WHERE NOT d.is_deleted AND d.document_id = ANY($1) \
             ORDER BY d.uploaded_date DESC",
        )
        .bind(&ids)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(docs)
    }

    // ==========================================
    // ADMIN API & CRUD OPERATIONS
    // ==========================================

    /// Atomically creates a document with optional flat assignment and metadata in a single transaction.
    /// If any step fails, everything is rolled back — no partial writes.
    #[allow(clippy::too_many_arguments)]
    pub async fn admin_create_document_atomic(
        &self,
        title: &str,
        description: Option<&str>,
        category: DocumentCategory,
        document_type: DocumentType,
        access_level: DocumentAccessLevel,
        flat: Option<&FlatIdentifier>,
        metadata: Option<MetadataInput<'_>>,
    ) -> Result<DocumentWithMetadata, String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let doc = Self::insert_document(
            &mut *tx,
            title,
            description,
            category,
            document_type,
            access_level,
            false,
            None,
        )
        .await?;

        if let Some(flat_id) = flat {
            Self::assign_flat_with_executor(&mut *tx, doc.document_id, flat_id).await?;
        }

        let meta = if let Some(m) = metadata {
            Some(
                Self::insert_metadata(
                    &mut *tx,
                    doc.document_id,
                    m.subcategory,
                    m.tags,
                    m.document_date,
                    m.valid_from,
                    m.valid_until,
                )
                .await?,
            )
        } else {
            None
        };

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(DocumentWithMetadata {
            document: doc,
            metadata: meta,
            download_url: None,
        })
    }

    /// Atomically updates a document with flat re-assignment and metadata upsert in a single transaction.
    /// Note: soft-deletion is handled by the dedicated `admin_soft_delete_document` endpoint.
    #[allow(clippy::too_many_arguments)]
    pub async fn admin_update_document_atomic(
        &self,
        document_id: Uuid,
        title: &str,
        description: Option<&str>,
        category: DocumentCategory,
        document_type: DocumentType,
        access_level: DocumentAccessLevel,
        flat: Option<&FlatIdentifier>,
        metadata: Option<MetadataInput<'_>>,
    ) -> Result<DocumentWithMetadata, String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let doc = Self::update_document(
            &mut *tx,
            document_id,
            title,
            description,
            category,
            document_type,
            access_level,
        )
        .await?;

        // Clear old flat mappings and re-assign within the same transaction
        Self::clear_flat_mappings_with_executor(&mut *tx, document_id).await?;
        if let Some(flat_id) = flat {
            Self::assign_flat_with_executor(&mut *tx, document_id, flat_id).await?;
        }

        let meta = if let Some(m) = metadata {
            Some(
                Self::upsert_metadata(
                    &mut *tx,
                    document_id,
                    m.subcategory,
                    m.tags,
                    m.document_date,
                    m.valid_from,
                    m.valid_until,
                )
                .await?,
            )
        } else {
            None
        };

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(DocumentWithMetadata {
            document: doc,
            metadata: meta,
            download_url: None,
        })
    }

    /// Atomically creates a user-uploaded document linked to their flat
    pub async fn user_create_document_atomic(
        &self,
        user: &authentication::User,
        title: &str,
        description: Option<&str>,
    ) -> Result<DocumentWithMetadata, String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        // Find the user's active flat_id
        let flat_id_opt = user
            .get_active_flat_id(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let flat_id =
            flat_id_opt.ok_or_else(|| "User does not have an active flat ownership".to_string())?;

        let doc = Self::insert_document(
            &mut *tx,
            title,
            description,
            DocumentCategory::ResidentSpecific,
            DocumentType::ResidentGeneral,
            DocumentAccessLevel::Private,
            true, // is_user_uploaded
            None,
        )
        .await?;

        // Insert into document_flats directly since we already have the flat_id
        sqlx::query("INSERT INTO document_flats (document_id, flat_id) VALUES ($1, $2)")
            .bind(doc.document_id)
            .bind(flat_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(DocumentWithMetadata {
            document: doc,
            metadata: None,
            download_url: None,
        })
    }

    /// Atomically creates a document for a process, assigning it to the process owner's flat.
    pub async fn create_document_for_process_atomic(
        &self,
        process_id: Uuid,
        title: &str,
        description: Option<&str>,
    ) -> Result<DocumentWithMetadata, String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        // Find the user_id associated with the process
        let process_user_id: i32 =
            sqlx::query_scalar("SELECT user_id FROM processes WHERE process_id = $1")
                .bind(process_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "Process not found".to_string())?;

        // Find that user's active flat_id
        let flat_id: i32 = sqlx::query_scalar(
            "SELECT flat_id FROM flat_ownerships WHERE user_id = $1 AND is_active = TRUE LIMIT 1",
        )
        .bind(process_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Process owner does not have an active flat ownership".to_string())?;

        let doc = Self::insert_document(
            &mut *tx,
            title,
            description,
            DocumentCategory::ProcessSpecific,
            DocumentType::ProcessGeneral,
            DocumentAccessLevel::Private,
            true, // is_user_uploaded
            Some(process_id),
        )
        .await?;

        // Insert into document_flats
        sqlx::query("INSERT INTO document_flats (document_id, flat_id) VALUES ($1, $2)")
            .bind(doc.document_id)
            .bind(flat_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(DocumentWithMetadata {
            document: doc,
            metadata: None,
            download_url: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_document<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        title: &str,
        description: Option<&str>,
        category: DocumentCategory,
        document_type: DocumentType,
        access_level: DocumentAccessLevel,
        is_user_uploaded: bool,
        process_id: Option<Uuid>,
    ) -> Result<Document, String> {
        let empty_roles: Vec<UserRole> = vec![];
        let doc: Document = sqlx::query_as(
            "INSERT INTO documents (title, description, category, document_type, access_level, restricted_committee_roles, is_user_uploaded, process_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING document_id, title, description, category, document_type, access_level, restricted_committee_roles, uploaded_date, is_deleted, is_user_uploaded, process_id",
        )
        .bind(title)
        .bind(description)
        .bind(category)
        .bind(document_type)
        .bind(access_level)
        .bind(&empty_roles)
        .bind(is_user_uploaded)
        .bind(process_id)
        .fetch_one(executor)
        .await
        .map_err(|e| e.to_string())?;

        Ok(doc)
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_document<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        title: &str,
        description: Option<&str>,
        category: DocumentCategory,
        document_type: DocumentType,
        access_level: DocumentAccessLevel,
    ) -> Result<Document, String> {
        let empty_roles: Vec<UserRole> = vec![];
        let doc: Document = sqlx::query_as(
            "UPDATE documents \
             SET title = $1, description = $2, category = $3, document_type = $4, access_level = $5, restricted_committee_roles = $6 \
             WHERE document_id = $7 \
             RETURNING document_id, title, description, category, document_type, access_level, restricted_committee_roles, uploaded_date, is_deleted, is_user_uploaded, process_id",
        )
        .bind(title)
        .bind(description)
        .bind(category)
        .bind(document_type)
        .bind(access_level)
        .bind(&empty_roles)
        .bind(document_id)
        .fetch_one(executor)
        .await
        .map_err(|e| e.to_string())?;

        Ok(doc)
    }

    async fn insert_metadata<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        subcategory: Option<&str>,
        tags: &[String],
        document_date: NaiveDate,
        valid_from: Option<NaiveDate>,
        valid_until: Option<NaiveDate>,
    ) -> Result<GenericDocumentMetadata, String> {
        let meta: GenericDocumentMetadata = sqlx::query_as(
            "INSERT INTO generic_document_metadata (document_id, subcategory, tags, document_date, valid_from, valid_until) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING document_id, subcategory, tags, document_date, valid_from, valid_until",
        )
        .bind(document_id)
        .bind(subcategory)
        .bind(tags)
        .bind(document_date)
        .bind(valid_from)
        .bind(valid_until)
        .fetch_one(executor)
        .await
        .map_err(|e| e.to_string())?;

        Ok(meta)
    }

    async fn upsert_metadata<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        subcategory: Option<&str>,
        tags: &[String],
        document_date: NaiveDate,
        valid_from: Option<NaiveDate>,
        valid_until: Option<NaiveDate>,
    ) -> Result<GenericDocumentMetadata, String> {
        let meta: GenericDocumentMetadata = sqlx::query_as(
            "INSERT INTO generic_document_metadata (document_id, subcategory, tags, document_date, valid_from, valid_until) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (document_id) DO UPDATE \
             SET subcategory = EXCLUDED.subcategory, tags = EXCLUDED.tags, document_date = EXCLUDED.document_date, \
                 valid_from = EXCLUDED.valid_from, valid_until = EXCLUDED.valid_until \
             RETURNING document_id, subcategory, tags, document_date, valid_from, valid_until",
        )
        .bind(document_id)
        .bind(subcategory)
        .bind(tags)
        .bind(document_date)
        .bind(valid_from)
        .bind(valid_until)
        .fetch_one(executor)
        .await
        .map_err(|e| e.to_string())?;

        Ok(meta)
    }

    async fn clear_flat_mappings_with_executor<
        'e,
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    >(
        executor: E,
        document_id: Uuid,
    ) -> Result<(), String> {
        sqlx::query("DELETE FROM document_flats WHERE document_id = $1")
            .bind(document_id)
            .execute(executor)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Resolves a FlatIdentifier to a flat_id and inserts a document_flats mapping
    /// in a single SQL statement, compatible with any sqlx Executor (including transactions).
    async fn assign_flat_with_executor<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        flat: &FlatIdentifier,
    ) -> Result<(), String> {
        // Combined lookup + insert in a single statement to avoid consuming the executor twice
        let result = sqlx::query(
            "INSERT INTO document_flats (document_id, flat_id) \
             SELECT $1, flat_id FROM flats WHERE wing = $2 AND flat_number = $3 \
             ON CONFLICT DO NOTHING",
        )
        .bind(document_id)
        .bind(flat.wing())
        .bind(flat.flat_number())
        .execute(executor)
        .await
        .map_err(|e| e.to_string())?;

        if result.rows_affected() == 0 {
            return Err(format!("Flat not found: {}", flat));
        }

        Ok(())
    }

    // ==========================================
    // STANDALONE ADMIN OPERATIONS (no transaction needed)
    // ==========================================

    /// Fetches any document (including soft-deleted ones) by its ID without user-level access filtration.
    pub async fn admin_get_any_document(
        &self,
        document_id: Uuid,
    ) -> Result<Option<DocumentWithMetadata>, String> {
        let mut doc_with_meta: Option<DocumentWithMetadata> = sqlx::query_as(
            "SELECT \
                d.document_id, d.title, d.description, d.category, d.document_type, d.access_level, d.restricted_committee_roles, d.uploaded_date, d.is_deleted, d.is_user_uploaded, d.process_id, \
                g.subcategory, g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             WHERE d.document_id = $1",
        )
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        if let Some(ref mut d) = doc_with_meta {
            let url = self.generate_presigned_url(&d.document).await?;
            d.download_url = Some(url);
        }

        Ok(doc_with_meta)
    }

    /// Lists all documents (including soft-deleted ones) in the database without filtration.
    pub async fn admin_list_all_documents(&self) -> Result<Vec<DocumentWithMetadata>, String> {
        let all_docs: Vec<DocumentWithMetadata> = sqlx::query_as(
            "SELECT \
                d.document_id, d.title, d.description, d.category, d.document_type, d.access_level, d.restricted_committee_roles, d.uploaded_date, d.is_deleted, d.is_user_uploaded, d.process_id, \
                g.subcategory, g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             ORDER BY d.uploaded_date DESC",
        )
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(all_docs)
    }

    /// Soft deletes a document by toggling is_deleted = TRUE.
    pub async fn admin_soft_delete_document(&self, document_id: Uuid) -> Result<(), String> {
        sqlx::query("UPDATE documents SET is_deleted = TRUE WHERE document_id = $1")
            .bind(document_id)
            .execute(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Fetches all non-deleted documents for a specific process.
    pub async fn get_documents_for_process<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        process_id: Uuid,
    ) -> Result<Vec<Document>, String> {
        sqlx::query_as(
            "SELECT document_id, title, description, category, document_type, access_level, restricted_committee_roles, uploaded_date, is_deleted, is_user_uploaded, process_id \
             FROM documents \
             WHERE process_id = $1 AND NOT is_deleted"
        )
        .bind(process_id)
        .fetch_all(executor)
        .await
        .map_err(|e| e.to_string())
    }

    /// Permanently deletes a document from the database (e.g. on S3 upload failure rollback).
    pub async fn delete_document_permanently(&self, document_id: Uuid) -> Result<(), String> {
        sqlx::query("DELETE FROM documents WHERE document_id = $1")
            .bind(document_id)
            .execute(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Fetches a non-deleted document by document_id that belongs to a specific process.
    pub async fn get_process_document(
        &self,
        document_id: Uuid,
        process_id: Uuid,
    ) -> Result<Option<Document>, String> {
        sqlx::query_as(
            "SELECT document_id, title, description, category, document_type, access_level, \
             restricted_committee_roles, uploaded_date, is_deleted, is_user_uploaded, process_id \
             FROM documents WHERE document_id = $1 AND process_id = $2 AND NOT is_deleted",
        )
        .bind(document_id)
        .bind(process_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }
}
