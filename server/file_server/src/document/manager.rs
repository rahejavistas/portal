use crate::document::{Document, DocumentWithMetadata, GenericDocumentMetadata, MetadataInput};
use base::flat::FlatIdentifier;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct DocumentManager {
    pub pg_pool: PgPool,
    pub s3_accessor: base::s3::S3Accessor,
}

/// Shared input for `admin_create_upload_intent`/`admin_create_upload_intent_by_id`.
pub struct AdminUploadIntentInput<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub category: &'a str,
    pub metadata: Option<MetadataInput<'a>>,
    pub content_type: &'a str,
    pub uploaded_by: Option<i32>,
}

/// Input for `update_document`.
pub struct UpdateDocumentInput<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub category: &'a str,
    pub metadata: Option<MetadataInput<'a>>,
}

impl DocumentManager {
    pub fn new(pg_pool: PgPool, s3_accessor: base::s3::S3Accessor) -> Self {
        Self {
            pg_pool,
            s3_accessor,
        }
    }

    pub async fn generate_presigned_url(&self, doc: &Document) -> Result<String, String> {
        let key = super::derive_s3_key(doc.document_id);
        self.s3_accessor
            .create_presigned_url(&key)
            .await
            .map_err(|e| e.to_string())
    }

    /// Fetches all allowed document IDs for a user based on category, designations, and flat ownership.
    pub async fn get_allowed_document_ids(
        &self,
        user: &authentication::User,
    ) -> Result<std::collections::HashSet<Uuid>, String> {
        let mut allowed_ids = std::collections::HashSet::new();

        if user.has_permission(authentication::Permission::DocumentsAllManage) {
            let admin_ids: Vec<Uuid> =
                sqlx::query_scalar("SELECT document_id FROM documents WHERE NOT is_deleted")
                    .fetch_all(&self.pg_pool)
                    .await
                    .map_err(|e| e.to_string())?;
            return Ok(admin_ids.into_iter().collect());
        }

        // 1. Documents in any is_public_read category are visible to everyone
        let public_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT d.document_id FROM documents d \
             JOIN document_categories dc ON dc.code = d.category \
             WHERE dc.is_public_read = TRUE AND NOT d.is_deleted",
        )
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        allowed_ids.extend(public_ids);

        // 2. Documents whose category is read-mapped to one of the user's active designations
        let designation_mapped_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT d.document_id \
             FROM documents d \
             JOIN document_category_designations dcd ON dcd.category = d.category \
             JOIN user_designations ud ON ud.designation_id = dcd.designation_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE AND dcd.can_read = TRUE AND NOT d.is_deleted",
        )
        .bind(user.user_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        allowed_ids.extend(designation_mapped_ids);

        // 3. Documents assigned to flat(s) owned/occupied by the user (RESIDENT_SPECIFIC)
        let owned_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT df.document_id \
             FROM document_flats df \
             JOIN flat_ownerships fo ON df.flat_id = fo.flat_id \
             WHERE fo.user_id = $1 AND fo.is_active = TRUE",
        )
        .bind(user.user_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        allowed_ids.extend(owned_ids);

        Ok(allowed_ids)
    }

    /// Fetches a non-deleted document by its ID and validates if the user has authorization to view it.
    /// Returns `Err("FORBIDDEN")` if access control evaluation fails.
    pub async fn get_document(
        &self,
        document_id: Uuid,
        user: &authentication::User,
    ) -> Result<Option<DocumentWithMetadata>, String> {
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

        let allowed_ids = self.get_allowed_document_ids(user).await?;
        if !allowed_ids.contains(&document_id) {
            return Err("FORBIDDEN".to_string());
        }

        let mut doc: Option<DocumentWithMetadata> = sqlx::query_as(
            "SELECT \
                d.document_id, d.title, d.description, d.category, d.uploaded_date, d.updated_at, d.is_deleted, d.is_user_uploaded, d.is_upload_confirmed, d.uploaded_by, \
                g.subcategory_id, sc.code AS subcategory_code, sc.label AS subcategory_label, \
                g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             LEFT JOIN document_subcategories sc ON g.subcategory_id = sc.subcategory_id \
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
                d.document_id, d.title, d.description, d.category, d.uploaded_date, d.updated_at, d.is_deleted, d.is_user_uploaded, d.is_upload_confirmed, d.uploaded_by, \
                g.subcategory_id, sc.code AS subcategory_code, sc.label AS subcategory_label, \
                g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             LEFT JOIN document_subcategories sc ON g.subcategory_id = sc.subcategory_id \
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

    /// Atomically creates an unconfirmed document with optional flat assignment and metadata,
    /// and returns a presigned PUT URL for direct S3 upload.
    pub async fn admin_create_upload_intent(
        &self,
        input: AdminUploadIntentInput<'_>,
        flat: Option<&FlatIdentifier>,
    ) -> Result<(DocumentWithMetadata, String, String), String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let doc = Self::insert_document(
            &mut *tx,
            input.title,
            input.description,
            input.category,
            false,
            false, // is_upload_confirmed: admin uploads use presigned URLs now
            input.uploaded_by,
        )
        .await?;

        if let Some(flat_id) = flat {
            Self::assign_flat(&mut *tx, doc.document_id, flat_id).await?;
        }

        let meta = if let Some(m) = input.metadata {
            Some(
                Self::insert_metadata(
                    &mut *tx,
                    doc.document_id,
                    m.subcategory_id,
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

        let key = super::derive_s3_key(doc.document_id);
        let upload_url = self
            .s3_accessor
            .create_presigned_put_url(&key, input.content_type)
            .await
            .map_err(|e| format!("Failed to generate presigned PUT URL: {}", e))?;

        Ok((
            DocumentWithMetadata {
                document: doc,
                metadata: meta,
                flat: None,
                download_url: None,
            },
            upload_url,
            key,
        ))
    }

    /// Atomically creates an unconfirmed document with optional flat ID assignment
    /// and returns a presigned PUT URL for direct S3 upload.
    pub async fn admin_create_upload_intent_by_id(
        &self,
        input: AdminUploadIntentInput<'_>,
        flat_id: Option<i32>,
    ) -> Result<(DocumentWithMetadata, String, String), String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let doc = Self::insert_document(
            &mut *tx,
            input.title,
            input.description,
            input.category,
            false,
            false,
            input.uploaded_by,
        )
        .await?;

        if let Some(fid) = flat_id {
            Self::assign_flat_by_id(&mut *tx, doc.document_id, fid).await?;
        }

        let meta = if let Some(m) = input.metadata {
            Some(
                Self::insert_metadata(
                    &mut *tx,
                    doc.document_id,
                    m.subcategory_id,
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

        let key = super::derive_s3_key(doc.document_id);
        let upload_url = self
            .s3_accessor
            .create_presigned_put_url(&key, input.content_type)
            .await
            .map_err(|e| format!("Failed to generate presigned PUT URL: {}", e))?;

        Ok((
            DocumentWithMetadata {
                document: doc,
                metadata: meta,
                flat: None,
                download_url: None,
            },
            upload_url,
            key,
        ))
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

        let flat_ids = user
            .get_active_flat_ids(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if flat_ids.is_empty() {
            return Err("User does not have an active flat ownership".to_string());
        }

        let doc = Self::insert_document(
            &mut *tx,
            title,
            description,
            super::RESIDENT_SPECIFIC_CATEGORY,
            true, // is_user_uploaded
            true, // is_upload_confirmed: server-proxied upload
            Some(user.user_id),
        )
        .await?;

        for flat_id in &flat_ids {
            sqlx::query("INSERT INTO document_flats (document_id, flat_id) VALUES ($1, $2)")
                .bind(doc.document_id)
                .bind(flat_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(DocumentWithMetadata {
            document: doc,
            metadata: None,
            flat: None,
            download_url: None,
        })
    }

    pub(super) async fn insert_document<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        title: &str,
        description: Option<&str>,
        category: &str,
        is_user_uploaded: bool,
        is_upload_confirmed: bool,
        uploaded_by: Option<i32>,
    ) -> Result<Document, String> {
        let doc: Document = sqlx::query_as(
            "INSERT INTO documents (title, description, category, is_user_uploaded, is_upload_confirmed, uploaded_by) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING document_id, title, description, category, uploaded_date, updated_at, is_deleted, is_user_uploaded, is_upload_confirmed, uploaded_by",
        )
        .bind(title)
        .bind(description)
        .bind(category)
        .bind(is_user_uploaded)
        .bind(is_upload_confirmed)
        .bind(uploaded_by)
        .fetch_one(executor)
        .await
        .map_err(|e| e.to_string())?;

        Ok(doc)
    }

    async fn insert_metadata<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        subcategory_id: Option<i32>,
        tags: &[String],
        document_date: NaiveDate,
        valid_from: Option<NaiveDate>,
        valid_until: Option<NaiveDate>,
    ) -> Result<GenericDocumentMetadata, String> {
        let row: (
            Uuid,
            Option<i32>,
            Vec<String>,
            NaiveDate,
            Option<NaiveDate>,
            Option<NaiveDate>,
        ) = sqlx::query_as(
            "INSERT INTO generic_document_metadata (document_id, subcategory_id, tags, document_date, valid_from, valid_until) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING document_id, subcategory_id, tags, document_date, valid_from, valid_until",
        )
        .bind(document_id)
        .bind(subcategory_id)
        .bind(tags)
        .bind(document_date)
        .bind(valid_from)
        .bind(valid_until)
        .fetch_one(executor)
        .await
        .map_err(|e| e.to_string())?;

        Ok(GenericDocumentMetadata {
            document_id: row.0,
            subcategory_id: row.1,
            subcategory_code: None,
            subcategory_label: None,
            tags: row.2,
            document_date: row.3,
            valid_from: row.4,
            valid_until: row.5,
        })
    }

    async fn assign_flat<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        flat: &FlatIdentifier,
    ) -> Result<(), String> {
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

    async fn assign_flat_by_id<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        document_id: Uuid,
        flat_id: i32,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO document_flats (document_id, flat_id) \
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(document_id)
        .bind(flat_id)
        .execute(executor)
        .await
        .map_err(|e| e.to_string())?;

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
                d.document_id, d.title, d.description, d.category, d.uploaded_date, d.updated_at, d.is_deleted, d.is_user_uploaded, d.is_upload_confirmed, d.uploaded_by, \
                g.subcategory_id, sc.code AS subcategory_code, sc.label AS subcategory_label, \
                g.tags, g.document_date, g.valid_from, g.valid_until \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             LEFT JOIN document_subcategories sc ON g.subcategory_id = sc.subcategory_id \
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
                d.document_id, d.title, d.description, d.category, d.uploaded_date, d.updated_at, d.is_deleted, d.is_user_uploaded, d.is_upload_confirmed, d.uploaded_by, \
                g.subcategory_id, sc.code AS subcategory_code, sc.label AS subcategory_label, \
                g.tags, g.document_date, g.valid_from, g.valid_until, \
                f.wing, f.flat_number \
             FROM documents d \
             LEFT JOIN generic_document_metadata g ON d.document_id = g.document_id \
             LEFT JOIN document_subcategories sc ON g.subcategory_id = sc.subcategory_id \
             LEFT JOIN LATERAL ( \
                SELECT fl.wing, fl.flat_number FROM document_flats df \
                JOIN flats fl ON fl.flat_id = df.flat_id \
                WHERE df.document_id = d.document_id \
                LIMIT 1 \
             ) f ON TRUE \
             ORDER BY d.uploaded_date DESC",
        )
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(all_docs)
    }

    /// Updates a document's title/description/category and replaces its metadata
    /// (upserted if `Some`, cleared if `None`). Bumps `updated_at`.
    pub async fn update_document(
        &self,
        document_id: Uuid,
        input: UpdateDocumentInput<'_>,
    ) -> Result<DocumentWithMetadata, String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let result = sqlx::query(
            "UPDATE documents SET title = $1, description = $2, category = $3, updated_at = NOW() \
             WHERE document_id = $4",
        )
        .bind(input.title)
        .bind(input.description)
        .bind(input.category)
        .bind(document_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        if result.rows_affected() == 0 {
            return Err("Document not found".to_string());
        }

        match input.metadata {
            Some(m) => {
                sqlx::query(
                    "INSERT INTO generic_document_metadata (document_id, subcategory_id, tags, document_date, valid_from, valid_until) \
                     VALUES ($1, $2, $3, $4, $5, $6) \
                     ON CONFLICT (document_id) DO UPDATE SET \
                        subcategory_id = EXCLUDED.subcategory_id, tags = EXCLUDED.tags, \
                        document_date = EXCLUDED.document_date, valid_from = EXCLUDED.valid_from, \
                        valid_until = EXCLUDED.valid_until",
                )
                .bind(document_id)
                .bind(m.subcategory_id)
                .bind(m.tags)
                .bind(m.document_date)
                .bind(m.valid_from)
                .bind(m.valid_until)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
            None => {
                sqlx::query("DELETE FROM generic_document_metadata WHERE document_id = $1")
                    .bind(document_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        self.admin_get_any_document(document_id)
            .await?
            .ok_or_else(|| "Document not found".to_string())
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

    /// Fetches all non-deleted documents linked to a specific process via process_fields.
    pub async fn get_documents_for_process<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        executor: E,
        process_id: Uuid,
    ) -> Result<Vec<Document>, String> {
        sqlx::query_as(
            "SELECT DISTINCT d.document_id, d.title, d.description, d.category, \
             d.uploaded_date, d.updated_at, d.is_deleted, d.is_user_uploaded, d.is_upload_confirmed, d.uploaded_by \
             FROM documents d \
             JOIN process_fields pf ON d.document_id = pf.document_id \
             JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
             WHERE ps.process_id = $1 AND NOT d.is_deleted",
        )
        .bind(process_id)
        .fetch_all(executor)
        .await
        .map_err(|e| e.to_string())
    }

    // ==========================================
    // PRESIGNED UPLOAD FLOW
    // ==========================================

    /// Creates an unconfirmed document record in the DB and generates a presigned PUT URL
    /// for direct client-to-S3 upload. The document is linked to the user's flat.
    ///
    /// Returns (DocumentWithMetadata, upload_url, s3_key).
    pub async fn create_upload_intent(
        &self,
        user: &authentication::User,
        title: &str,
        content_type: &str,
    ) -> Result<(DocumentWithMetadata, String, String), String> {
        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let flat_ids = user
            .get_active_flat_ids(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if flat_ids.is_empty() {
            return Err("User does not have an active flat ownership".to_string());
        }

        let doc = Self::insert_document(
            &mut *tx,
            title,
            None,
            super::RESIDENT_SPECIFIC_CATEGORY,
            true,  // is_user_uploaded
            false, // is_upload_confirmed: NOT confirmed until client finishes S3 upload
            Some(user.user_id),
        )
        .await?;

        for flat_id in &flat_ids {
            sqlx::query("INSERT INTO document_flats (document_id, flat_id) VALUES ($1, $2)")
                .bind(doc.document_id)
                .bind(flat_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        let key = super::derive_s3_key(doc.document_id);
        let upload_url = self
            .s3_accessor
            .create_presigned_put_url(&key, content_type)
            .await
            .map_err(|e| format!("Failed to generate presigned PUT URL: {}", e))?;

        Ok((
            DocumentWithMetadata {
                document: doc,
                metadata: None,
                flat: None,
                download_url: None,
            },
            upload_url,
            key,
        ))
    }

    /// Confirms that a previously created upload intent was completed by checking
    /// that the file actually exists on S3 via a HEAD request, then sets
    /// `is_upload_confirmed = true` in the DB.
    pub async fn confirm_upload(&self, document_id: Uuid) -> Result<(), String> {
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT is_upload_confirmed FROM documents WHERE document_id = $1 AND NOT is_deleted",
        )
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        match row {
            None => return Err("Document not found".to_string()),
            Some((true,)) => return Ok(()),
            Some((false,)) => {}
        }

        let key = super::derive_s3_key(document_id);
        let head_result = self
            .s3_accessor
            .head_object(&key)
            .await
            .map_err(|e| format!("S3 HEAD check failed: {}", e))?;

        match head_result {
            None => {
                return Err("File not found on S3. Upload may not have completed.".to_string());
            }
            Some(size) if size > 1024 * 1024 * 1024 => {
                return Err(format!(
                    "Uploaded file exceeds the 1GB limit ({} bytes). Upload rejected.",
                    size
                ));
            }
            Some(_) => {}
        }

        sqlx::query("UPDATE documents SET is_upload_confirmed = TRUE WHERE document_id = $1")
            .bind(document_id)
            .execute(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}
