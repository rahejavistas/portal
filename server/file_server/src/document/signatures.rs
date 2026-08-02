use super::{Document, DocumentManager};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "signature_status", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum SignatureStatus {
    Pending,
    Completed,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SignatureRequirement {
    pub requirement_id: i32,
    pub document_id: Uuid,
    pub signed_document_id: Option<Uuid>,
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub template_field_id: i32,
    pub signer_id: i32,
    pub sign_order: i32,
    pub provider: String,
    pub external_document_id: Option<String>,
    pub external_signer_id: Option<String>,
    /// The provider's own signing-page link for this signer, if any — lets the UI
    /// offer "open signing page" directly.
    pub sign_url: Option<String>,
    pub status: SignatureStatus,
    pub signed_by: Option<i32>,
    pub signed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    // Joined display fields, not present on the raw table.
    pub document_title: String,
    pub document_category: String,
    pub signer_user_id: i32,
    pub signer_name: String,
    pub signer_email: String,
    pub signer_phone: String,
}

const REQUIREMENT_SELECT: &str = "SELECT r.requirement_id, r.document_id, r.signed_document_id, \
    r.process_id, r.template_step_id, r.template_field_id, r.signer_id, r.sign_order, \
    r.provider, sl.external_document_id, sl.external_signer_id, sl.sign_url, r.status, r.signed_by, r.signed_at, r.created_at, r.updated_at, \
    d.title AS document_title, d.category AS document_category, \
    s.user_id AS signer_user_id, s.name AS signer_name, s.email AS signer_email, s.phone AS signer_phone \
    FROM document_signature_requirements r \
    LEFT JOIN signyu_requirement_links sl ON sl.requirement_id = r.requirement_id \
    JOIN documents d ON d.document_id = r.document_id \
    JOIN document_signers s ON s.signer_id = r.signer_id";

/// A requirement is actionable only once every lower-`sign_order` requirement for the
/// same document has been signed (COMPLETED).
const CURRENT_TURN_CLAUSE: &str = "NOT EXISTS ( \
    SELECT 1 FROM document_signature_requirements r2 \
    WHERE r2.document_id = r.document_id AND r2.sign_order < r.sign_order AND r2.status = 'PENDING' \
)";

impl DocumentManager {
    /// PENDING requirements for `user_id` whose turn it currently is.
    pub async fn signature_queue_list_for_user(
        &self,
        user_id: i32,
    ) -> Result<Vec<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} WHERE r.status = 'PENDING' AND s.user_id = $1 AND {} ORDER BY r.created_at ASC",
            REQUIREMENT_SELECT, CURRENT_TURN_CLAUSE
        ))
        .bind(user_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Whether `user_id` is/was attached as a signer on any document in `process_id` —
    /// grants read-only access to that one process without full reviewer visibility.
    pub async fn signer_has_requirement_for_process(
        &self,
        user_id: i32,
        process_id: Uuid,
    ) -> Result<bool, String> {
        sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM document_signature_requirements r \
                 JOIN document_signers s ON s.signer_id = r.signer_id \
                 WHERE r.process_id = $1 AND s.user_id = $2 \
             )",
        )
        .bind(process_id)
        .bind(user_id)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Every requirement, for the admin document_id <-> signed_document_id lineage map.
    pub async fn signature_list_all(&self) -> Result<Vec<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} ORDER BY r.created_at DESC",
            REQUIREMENT_SELECT
        ))
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// A single requirement by requirement_id.
    pub async fn signature_get_requirement(
        &self,
        requirement_id: i32,
    ) -> Result<Option<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} WHERE r.requirement_id = $1",
            REQUIREMENT_SELECT
        ))
        .bind(requirement_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Multiple requirements by id in one round trip, in sign_order.
    pub async fn signature_get_requirements_batch(
        &self,
        requirement_ids: &[i32],
    ) -> Result<Vec<SignatureRequirement>, String> {
        if requirement_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as(&format!(
            "{} WHERE r.requirement_id = ANY($1) ORDER BY r.sign_order",
            REQUIREMENT_SELECT
        ))
        .bind(requirement_ids)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Every requirement for a document, in signing order.
    pub async fn signature_requirements_for_document(
        &self,
        document_id: Uuid,
    ) -> Result<Vec<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} WHERE r.document_id = $1 ORDER BY r.sign_order",
            REQUIREMENT_SELECT
        ))
        .bind(document_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Whether every requirement with a lower `sign_order` for the same document has
    /// been signed (COMPLETED) — i.e. it's this requirement's turn.
    pub async fn signature_is_current_turn(
        &self,
        requirement: &SignatureRequirement,
    ) -> Result<bool, String> {
        sqlx::query_scalar(
            "SELECT NOT EXISTS ( \
                 SELECT 1 FROM document_signature_requirements r2 \
                 WHERE r2.document_id = $1 AND r2.sign_order < $2 AND r2.status = 'PENDING' \
             )",
        )
        .bind(requirement.document_id)
        .bind(requirement.sign_order)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// What this signer should see: the original if first in chain, else the previous
    /// signer's copy.
    pub async fn input_document_id_for_requirement(
        &self,
        requirement: &SignatureRequirement,
    ) -> Result<Uuid, String> {
        let previous_signed: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT signed_document_id FROM document_signature_requirements \
             WHERE document_id = $1 AND sign_order < $2 \
             ORDER BY sign_order DESC LIMIT 1",
        )
        .bind(requirement.document_id)
        .bind(requirement.sign_order)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(previous_signed.flatten().unwrap_or(requirement.document_id))
    }

    /// Attaches signers in the given order (signer_ids[0] signs first). Idempotent per
    /// (document, signer) — re-attaching an existing signer is a no-op for them.
    #[allow(clippy::too_many_arguments)]
    pub async fn signature_add_signers_to_document(
        &self,
        document_id: Uuid,
        process_id: Uuid,
        template_step_id: i32,
        template_field_id: i32,
        signer_ids: &[i32],
    ) -> Result<Vec<SignatureRequirement>, String> {
        if signer_ids.is_empty() {
            return Err("At least one signer must be selected".to_string());
        }

        let existing_max_order: Option<i32> = sqlx::query_scalar(
            "SELECT MAX(sign_order) FROM document_signature_requirements WHERE document_id = $1",
        )
        .bind(document_id)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        let start_order = existing_max_order.unwrap_or(0) + 1;

        let sign_orders: Vec<i32> = (start_order..start_order + signer_ids.len() as i32).collect();

        let created_ids: Vec<i32> = sqlx::query_scalar(
            "INSERT INTO document_signature_requirements \
                (document_id, process_id, template_step_id, template_field_id, signer_id, sign_order) \
             SELECT $1, $2, $3, $4, v.signer_id, v.sign_order \
             FROM UNNEST($5::int[], $6::int[]) AS v(signer_id, sign_order) \
             ON CONFLICT (document_id, signer_id) DO NOTHING \
             RETURNING requirement_id",
        )
        .bind(document_id)
        .bind(process_id)
        .bind(template_step_id)
        .bind(template_field_id)
        .bind(signer_ids)
        .bind(&sign_orders)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        if created_ids.is_empty() {
            return Ok(vec![]);
        }

        // ORDER BY sign_order: callers zip this against a per-signer response list.
        sqlx::query_as(&format!(
            "{} WHERE r.requirement_id = ANY($1) ORDER BY r.sign_order",
            REQUIREMENT_SELECT
        ))
        .bind(&created_ids)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Records N requirements as now tracked by an external provider (SignYu), in 2
    /// statements total. Parallel arrays, same order, all sharing `external_document_id`.
    pub async fn signature_set_external_tracking_batch(
        &self,
        requirement_ids: &[i32],
        provider: &str,
        external_document_id: &str,
        external_signer_ids: &[String],
        sign_urls: &[Option<String>],
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE document_signature_requirements SET provider = $1, updated_at = NOW() \
             WHERE requirement_id = ANY($2)",
        )
        .bind(provider)
        .bind(requirement_ids)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "INSERT INTO signyu_requirement_links (requirement_id, external_document_id, external_signer_id, sign_url) \
             SELECT v.requirement_id, $1, v.external_signer_id, v.sign_url \
             FROM UNNEST($2::int[], $3::text[], $4::text[]) AS v(requirement_id, external_signer_id, sign_url) \
             ON CONFLICT (requirement_id) DO UPDATE SET \
                 external_document_id = EXCLUDED.external_document_id, \
                 external_signer_id = EXCLUDED.external_signer_id, \
                 sign_url = EXCLUDED.sign_url",
        )
        .bind(external_document_id)
        .bind(requirement_ids)
        .bind(external_signer_ids)
        .bind(sign_urls)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Every requirement sharing one external_document_id, for bulk reconciliation.
    pub async fn signature_requirements_for_external_document(
        &self,
        external_document_id: &str,
    ) -> Result<Vec<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} WHERE sl.external_document_id = $1 ORDER BY r.sign_order",
            REQUIREMENT_SELECT
        ))
        .bind(external_document_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Marks a provider-tracked requirement COMPLETED, pointing it at the one shared
    /// signed PDF SignYu produces for all signers.
    pub async fn signature_complete_via_provider(
        &self,
        requirement_id: i32,
        signed_document_id: Uuid,
    ) -> Result<(), String> {
        let requirement = self
            .signature_get_requirement(requirement_id)
            .await?
            .ok_or_else(|| "Signature requirement not found".to_string())?;

        sqlx::query(
            "UPDATE document_signature_requirements \
             SET status = 'COMPLETED', signed_document_id = $1, signed_by = $2, \
                 signed_at = NOW(), updated_at = NOW() \
             WHERE requirement_id = $3 AND status = 'PENDING'",
        )
        .bind(signed_document_id)
        .bind(requirement.signer_user_id)
        .bind(requirement_id)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Clones `original`'s title/description/category/flat mapping into a new unconfirmed row.
    async fn clone_document_as_new_row(
        conn: &mut sqlx::PgConnection,
        original: &Document,
        content_type: &str,
        uploaded_by: Option<i32>,
    ) -> Result<Document, String> {
        let new_doc = Self::insert_document(
            &mut *conn,
            &original.title,
            original.description.as_deref(),
            &original.category,
            false,
            false,
            uploaded_by,
            Some(content_type),
        )
        .await?;

        if original.category == super::RESIDENT_SPECIFIC_CATEGORY {
            sqlx::query(
                "INSERT INTO document_flats (document_id, flat_id) \
                 SELECT $1, flat_id FROM document_flats WHERE document_id = $2",
            )
            .bind(new_doc.document_id)
            .bind(original.document_id)
            .execute(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
        }

        Ok(new_doc)
    }

    /// Stores the fully-signed PDF and marks every PENDING requirement for it
    /// COMPLETED. No-op if already reconciled.
    pub async fn signature_ingest_provider_completed_document(
        &self,
        external_document_id: &str,
        pdf_bytes: Vec<u8>,
    ) -> Result<Vec<SignatureRequirement>, String> {
        let requirements = self
            .signature_requirements_for_external_document(external_document_id)
            .await?;
        let pending_ids: Vec<i32> = requirements
            .iter()
            .filter(|r| r.status == SignatureStatus::Pending)
            .map(|r| r.requirement_id)
            .collect();
        if pending_ids.is_empty() {
            return Ok(requirements);
        }

        let original_document_id = requirements[0].document_id;
        let original = self
            .admin_get_any_document(original_document_id)
            .await?
            .ok_or_else(|| "Original document not found".to_string())?
            .document;

        let mut conn = self.pg_pool.acquire().await.map_err(|e| e.to_string())?;
        let new_doc =
            Self::clone_document_as_new_row(&mut conn, &original, "application/pdf", None).await?;
        drop(conn);

        let key = super::derive_s3_key(new_doc.document_id);
        self.s3_accessor
            .upload_file(&key, pdf_bytes.into(), "application/pdf")
            .await
            .map_err(|e| e.to_string())?;
        self.confirm_upload(new_doc.document_id).await?;

        for requirement_id in pending_ids {
            self.signature_complete_via_provider(requirement_id, new_doc.document_id)
                .await?;
        }

        self.signature_requirements_for_external_document(external_document_id)
            .await
    }

    /// Generates a presigned upload URL for a signed replacement of `requirement_id`'s
    /// document. Fails unless the requirement is still PENDING.
    pub async fn signature_replace_upload_url(
        &self,
        requirement_id: i32,
        content_type: &str,
        signer_user_id: i32,
    ) -> Result<(Uuid, String), String> {
        let requirement = self
            .signature_get_requirement(requirement_id)
            .await?
            .ok_or_else(|| "Signature requirement not found".to_string())?;

        if requirement.status != SignatureStatus::Pending {
            return Err(format!(
                "Signature requirement is already {:?}",
                requirement.status
            ));
        }

        if !self.signature_is_current_turn(&requirement).await? {
            return Err(
                "It is not this signer's turn yet — an earlier signer in the chain hasn't signed"
                    .to_string(),
            );
        }

        // Metadata always mirrors the TRUE original, not an intermediate signer's copy.
        let original = self
            .admin_get_any_document(requirement.document_id)
            .await?
            .ok_or_else(|| "Original document not found".to_string())?
            .document;

        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        // is_user_uploaded: false; is_upload_confirmed: false (not until the client
        // finishes the PUT).
        let new_doc =
            Self::clone_document_as_new_row(&mut tx, &original, content_type, Some(signer_user_id))
                .await?;

        tx.commit().await.map_err(|e| e.to_string())?;

        let key = super::derive_s3_key(new_doc.document_id);
        let upload_url = self
            .s3_accessor
            .create_presigned_put_url(&key, content_type)
            .await
            .map_err(|e| format!("Failed to generate presigned PUT URL: {}", e))?;

        Ok((new_doc.document_id, upload_url))
    }

    /// Marks the replacement upload COMPLETED. Returns the updated requirement and
    /// whether it was the last PENDING one for the document. `existing` is the
    /// caller's already-fetched copy, avoiding a redundant re-fetch here.
    pub async fn signature_replace_confirm_upload(
        &self,
        existing: &SignatureRequirement,
        new_document_id: Uuid,
        signer_user_id: i32,
    ) -> Result<(SignatureRequirement, bool), String> {
        self.confirm_upload(new_document_id).await?;

        if existing.status != SignatureStatus::Pending {
            return Err("Signature requirement is no longer pending".to_string());
        }

        let updated: Option<SignatureRequirement> = sqlx::query_as(&format!(
            "WITH updated AS ( \
                 UPDATE document_signature_requirements \
                 SET status = 'COMPLETED', signed_document_id = $1, signed_by = $2, \
                     signed_at = NOW(), updated_at = NOW() \
                 WHERE requirement_id = $3 AND status = 'PENDING' \
                 RETURNING * \
             ) {}",
            REQUIREMENT_SELECT.replacen(
                "FROM document_signature_requirements r",
                "FROM updated r",
                1
            )
        ))
        .bind(new_document_id)
        .bind(signer_user_id)
        .bind(existing.requirement_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let updated =
            updated.ok_or_else(|| "Signature requirement is no longer pending".to_string())?;

        let remaining_pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM document_signature_requirements \
             WHERE document_id = $1 AND status = 'PENDING'",
        )
        .bind(updated.document_id)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok((updated, remaining_pending == 0))
    }
}
