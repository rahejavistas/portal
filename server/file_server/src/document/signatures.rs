use super::DocumentManager;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "signature_status", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum SignatureStatus {
    Pending,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "signature_event_source", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum EventSource {
    Internal,
    External,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SignatureRequirement {
    pub requirement_id: i32,
    pub document_id: Uuid,
    pub signed_document_id: Option<Uuid>,
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub template_field_id: i32,
    pub provider: String,
    pub status: SignatureStatus,
    pub signed_by: Option<i32>,
    pub signed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    // Joined display fields, not present on the raw table.
    pub document_title: String,
    pub document_category: String,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SignatureEvent {
    pub event_id: i32,
    pub requirement_id: i32,
    pub source: EventSource,
    pub event_type: String,
    pub raw_payload: Option<serde_json::Value>,
    pub actor_user_id: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const REQUIREMENT_SELECT: &str = "SELECT r.requirement_id, r.document_id, r.signed_document_id, \
    r.process_id, r.template_step_id, r.template_field_id, \
    r.provider, r.status, r.signed_by, r.signed_at, r.created_at, r.updated_at, \
    d.title AS document_title, d.category AS document_category \
    FROM document_signature_requirements r \
    JOIN documents d ON d.document_id = r.document_id";

impl DocumentManager {
    /// All PENDING requirements — the shared signing queue.
    pub async fn signature_queue_list(&self) -> Result<Vec<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} WHERE r.status = 'PENDING' ORDER BY r.created_at ASC",
            REQUIREMENT_SELECT
        ))
        .fetch_all(&self.pg_pool)
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

    /// A single requirement + its event log, by requirement_id.
    pub async fn signature_get_requirement(
        &self,
        requirement_id: i32,
    ) -> Result<Option<(SignatureRequirement, Vec<SignatureEvent>)>, String> {
        let requirement: Option<SignatureRequirement> = sqlx::query_as(&format!(
            "{} WHERE r.requirement_id = $1",
            REQUIREMENT_SELECT
        ))
        .bind(requirement_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let Some(requirement) = requirement else {
            return Ok(None);
        };

        let events: Vec<SignatureEvent> = sqlx::query_as(
            "SELECT event_id, requirement_id, source, event_type, raw_payload, actor_user_id, created_at \
             FROM document_signature_events WHERE requirement_id = $1 ORDER BY created_at ASC",
        )
        .bind(requirement_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(Some((requirement, events)))
    }

    /// The active (non-cancelled) requirement for a document, if any.
    pub async fn signature_for_document(
        &self,
        document_id: Uuid,
    ) -> Result<Option<SignatureRequirement>, String> {
        sqlx::query_as(&format!(
            "{} WHERE r.document_id = $1 AND r.status <> 'CANCELLED'",
            REQUIREMENT_SELECT
        ))
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn signature_replace_upload_url(
        &self,
        document_id: Uuid,
        content_type: &str,
        signer_user_id: i32,
    ) -> Result<(Uuid, String), String> {
        let requirement = self
            .signature_for_document(document_id)
            .await?
            .ok_or_else(|| "No signature requirement for this document".to_string())?;

        if requirement.status != SignatureStatus::Pending {
            return Err(format!(
                "Signature requirement is already {:?}",
                requirement.status
            ));
        }

        let original = self
            .admin_get_any_document(document_id)
            .await?
            .ok_or_else(|| "Original document not found".to_string())?
            .document;

        let mut tx = base::postgres::begin_transaction(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;

        let new_doc = Self::insert_document(
            &mut *tx,
            &format!("{} (Signed)", original.title),
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
            .bind(document_id)
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

    pub async fn signature_replace_confirm_upload(
        &self,
        document_id: Uuid,
        new_document_id: Uuid,
        signer_user_id: i32,
    ) -> Result<SignatureRequirement, String> {
        self.confirm_upload(new_document_id).await?;

        let requirement_id: Option<i32> = sqlx::query_scalar(
            "UPDATE document_signature_requirements \
             SET status = 'COMPLETED', signed_document_id = $1, signed_by = $2, \
                 signed_at = NOW(), updated_at = NOW() \
             WHERE document_id = $3 AND status = 'PENDING' \
             RETURNING requirement_id",
        )
        .bind(new_document_id)
        .bind(signer_user_id)
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let requirement_id = requirement_id
            .ok_or_else(|| "Signature requirement is no longer pending".to_string())?;

        sqlx::query(
            "INSERT INTO document_signature_events (requirement_id, source, event_type, actor_user_id, raw_payload) \
             VALUES ($1, $2, 'document_replaced', $3, $4)",
        )
        .bind(requirement_id)
        .bind(EventSource::Internal)
        .bind(signer_user_id)
        .bind(serde_json::json!({ "signed_document_id": new_document_id }))
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        self.signature_get_requirement(requirement_id)
            .await?
            .map(|(req, _)| req)
            .ok_or_else(|| "Requirement vanished after update".to_string())
    }

    /// Admin-only override: cancels a PENDING requirement without ever signing it.
    pub async fn signature_cancel_requirement(
        &self,
        document_id: Uuid,
        actor_user_id: i32,
        notes: Option<&str>,
    ) -> Result<SignatureRequirement, String> {
        let requirement_id: Option<i32> = sqlx::query_scalar(
            "UPDATE document_signature_requirements \
             SET status = 'CANCELLED', updated_at = NOW() \
             WHERE document_id = $1 AND status = 'PENDING' \
             RETURNING requirement_id",
        )
        .bind(document_id)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let requirement_id = requirement_id
            .ok_or_else(|| "No pending signature requirement for this document".to_string())?;

        sqlx::query(
            "INSERT INTO document_signature_events (requirement_id, source, event_type, actor_user_id, raw_payload) \
             VALUES ($1, $2, 'requirement_cancelled', $3, $4)",
        )
        .bind(requirement_id)
        .bind(EventSource::Internal)
        .bind(actor_user_id)
        .bind(notes.map(|n| serde_json::json!({ "notes": n })))
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        self.signature_get_requirement(requirement_id)
            .await?
            .map(|(req, _)| req)
            .ok_or_else(|| "Requirement vanished after update".to_string())
    }
}
