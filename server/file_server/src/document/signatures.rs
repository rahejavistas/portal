use super::DocumentManager;
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
    /// PENDING requirements scoped to `user_id` AND whose turn it currently is — the
    /// queue as seen by a plain signer, who should only be offered documents waiting on
    /// them specifically, and only once everyone signing before them is done.
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

    /// Whether `user_id` is (or ever was) attached as a signer on ANY document within
    /// `process_id` — regardless of requirement status. Used to grant a signer
    /// read-only access to view the one specific process their signature concerns,
    /// without granting `process.reviewer`/`process.view`'s blanket visibility into
    /// every resident's processes.
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

    /// Every requirement regardless of status, for building a document_id <->
    /// signed_document_id lineage map in the admin document list (one call instead of
    /// a lookup per document).
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

    /// The document `requirement`'s signer should actually see/sign: the original for
    /// the first signer in the chain, otherwise the PREVIOUS signer's signed copy — so
    /// each signer builds on what the one before them produced, instead of every
    /// signer independently signing a blind copy of the untouched original.
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

    /// A process reviewer attaches one or more signers to a document from a specific
    /// process field, IN THE ORDER GIVEN — `signer_ids[0]` signs first, and later
    /// signers only become actionable once everyone before them has signed. Idempotent
    /// per (document, signer) — re-attaching an already PENDING/COMPLETED signer is a
    /// no-op for that signer (it keeps its original position in the chain).
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

        let mut created_ids = Vec::new();
        for (sign_order, signer_id) in (start_order..).zip(signer_ids.iter()) {
            let requirement_id: Option<i32> = sqlx::query_scalar(
                "INSERT INTO document_signature_requirements \
                    (document_id, process_id, template_step_id, template_field_id, signer_id, sign_order) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (document_id, signer_id) DO NOTHING \
                 RETURNING requirement_id",
            )
            .bind(document_id)
            .bind(process_id)
            .bind(template_step_id)
            .bind(template_field_id)
            .bind(signer_id)
            .bind(sign_order)
            .fetch_optional(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

            if let Some(requirement_id) = requirement_id {
                created_ids.push(requirement_id);
            }
        }

        let mut requirements = Vec::with_capacity(created_ids.len());
        for id in created_ids {
            if let Some(req) = self.signature_get_requirement(id).await? {
                requirements.push(req);
            }
        }
        Ok(requirements)
    }

    /// Records that `requirement_id` is now tracked by an external provider (SignYu),
    /// after successfully sending it out for signature there.
    pub async fn signature_set_external_tracking(
        &self,
        requirement_id: i32,
        provider: &str,
        external_document_id: &str,
        external_signer_id: &str,
        sign_url: Option<&str>,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE document_signature_requirements SET provider = $1, updated_at = NOW() WHERE requirement_id = $2",
        )
        .bind(provider)
        .bind(requirement_id)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            "INSERT INTO signyu_requirement_links (requirement_id, external_document_id, external_signer_id, sign_url) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (requirement_id) DO UPDATE SET \
                 external_document_id = EXCLUDED.external_document_id, \
                 external_signer_id = EXCLUDED.external_signer_id, \
                 sign_url = EXCLUDED.sign_url",
        )
        .bind(requirement_id)
        .bind(external_document_id)
        .bind(external_signer_id)
        .bind(sign_url)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Every SignYu-tracked requirement sharing one `external_document_id` — used to
    /// reconcile ALL of them at once when the provider reports completion.
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

    /// Marks a provider-tracked requirement COMPLETED once the whole external document
    /// is done, pointing it at `signed_document_id` (the downloaded, fully-signed PDF —
    /// shared by every requirement for the same document, since SignYu produces one
    /// final file for everyone, not a copy per signer). `signed_by` is the requirement's
    /// own signer's user account, since that's who actually signed, even though it
    /// happened at the external provider rather than through our own upload flow.
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

    /// Ingests the fully-signed PDF once an external provider reports the whole
    /// document COMPLETED: stores it as a new document, then marks every still-PENDING
    /// requirement sharing that `external_document_id` COMPLETED against it. Idempotent
    /// — a document with nothing left PENDING is a no-op (already reconciled).
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

        let new_doc = Self::insert_document(
            &self.pg_pool,
            &original.title,
            original.description.as_deref(),
            &original.category,
            false,
            false,
            None,
        )
        .await?;

        let key = super::derive_s3_key(new_doc.document_id);
        self.s3_accessor
            .upload_file(&key, pdf_bytes.into(), "application/pdf")
            .await
            .map_err(|e| e.to_string())?;
        self.confirm_upload(new_doc.document_id).await?;

        if original.category == super::RESIDENT_SPECIFIC_CATEGORY {
            sqlx::query(
                "INSERT INTO document_flats (document_id, flat_id) \
                 SELECT $1, flat_id FROM document_flats WHERE document_id = $2",
            )
            .bind(new_doc.document_id)
            .bind(original_document_id)
            .execute(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;
        }

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

        // Metadata (title/description/category/flat) always mirrors the TRUE original —
        // not whatever the previous signer's intermediate copy was titled — so the title
        // stays exactly what the resident/admin uploaded, with no "(Signed by X) (Signed
        // by Y)" suffix pile-up as a chain grows. The signer's own content (what they
        // actually upload) is a separate concern, handled by `input_document_id_for_requirement`
        // in `document_download_url` — that's what determines what they SEE to sign,
        // unrelated to what this new row is TITLED.
        let original = self
            .admin_get_any_document(requirement.document_id)
            .await?
            .ok_or_else(|| "Original document not found".to_string())?
            .document;

        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let new_doc = Self::insert_document(
            &mut *tx,
            &original.title,
            original.description.as_deref(),
            &original.category,
            false, // is_user_uploaded
            false, // is_upload_confirmed: not until the client finishes the PUT
            Some(signer_user_id),
        )
        .await?;

        if original.category == super::RESIDENT_SPECIFIC_CATEGORY {
            sqlx::query(
                "INSERT INTO document_flats (document_id, flat_id) \
                 SELECT $1, flat_id FROM document_flats WHERE document_id = $2",
            )
            .bind(new_doc.document_id)
            .bind(requirement.document_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        let key = super::derive_s3_key(new_doc.document_id);
        let upload_url = self
            .s3_accessor
            .create_presigned_put_url(&key, content_type)
            .await
            .map_err(|e| format!("Failed to generate presigned PUT URL: {}", e))?;

        Ok((new_doc.document_id, upload_url))
    }

    /// Confirms the signed-replacement upload for `requirement_id`, marking it
    /// COMPLETED. Returns the updated requirement plus whether this was the LAST
    /// outstanding (PENDING) requirement for the underlying document — the caller
    /// should only repoint the process field at the signed document when every
    /// attached signer has now signed.
    pub async fn signature_replace_confirm_upload(
        &self,
        requirement_id: i32,
        new_document_id: Uuid,
        signer_user_id: i32,
    ) -> Result<(SignatureRequirement, bool), String> {
        self.confirm_upload(new_document_id).await?;

        let existing = self
            .signature_get_requirement(requirement_id)
            .await?
            .ok_or_else(|| "Signature requirement not found".to_string())?;

        if existing.status != SignatureStatus::Pending {
            return Err("Signature requirement is no longer pending".to_string());
        }

        sqlx::query(
            "UPDATE document_signature_requirements \
             SET status = 'COMPLETED', signed_document_id = $1, signed_by = $2, \
                 signed_at = NOW(), updated_at = NOW() \
             WHERE requirement_id = $3 AND status = 'PENDING'",
        )
        .bind(new_document_id)
        .bind(signer_user_id)
        .bind(requirement_id)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let remaining_pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM document_signature_requirements \
             WHERE document_id = $1 AND status = 'PENDING'",
        )
        .bind(existing.document_id)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let updated = self
            .signature_get_requirement(requirement_id)
            .await?
            .ok_or_else(|| "Requirement vanished after update".to_string())?;

        Ok((updated, remaining_pending == 0))
    }
}
