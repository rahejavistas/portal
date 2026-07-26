//! The signer directory (`document_signers`): a curated list of users eligible to be
//! attached as e-signature signers on a process document, each with the contact info
//! (name/phone/email) an external signing provider needs. Kept separate from `users`
//! since not every user is a signer.

use super::DocumentManager;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Signer {
    pub signer_id: i32,
    pub user_id: i32,
    pub name: String,
    pub phone: String,
    pub email: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub updated_by: Option<i32>,
    // Joined display fields, not present on the raw table.
    pub user_email: String,
    /// Codes of the designations this user currently holds — lets the "Add Signers"
    /// picker filter to only the designation(s) a `required_signs_from_designations`
    /// field asks for, and lets the server re-validate that same requirement when a
    /// reviewer submits `add-signers` — see
    /// `handlers::process::actions::validate_signer_order_against_designations`.
    pub designation_codes: Vec<String>,
}

/// What a process reviewer sees in the "Add Signers" picker / signer directory
/// oversight view — deliberately excludes `phone`/`email` (the signer's own private
/// contact info, entered for the signing provider's use, not for a reviewer to browse).
/// Only the signer themselves ever sees their own phone/email, via
/// `GET /api/v1/profile/signer-info`.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SignerDirectoryEntry {
    pub signer_id: i32,
    pub user_id: i32,
    pub name: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub updated_by: Option<i32>,
    pub user_email: String,
    pub designation_codes: Vec<String>,
}

const SIGNER_SELECT: &str = "SELECT s.signer_id, s.user_id, s.name, s.phone, s.email, \
    s.is_active, s.created_at, s.updated_at, s.updated_by, u.email AS user_email, \
    COALESCE(( \
        SELECT array_agg(d.code) FROM user_designations ud \
        JOIN designations d ON d.designation_id = ud.designation_id \
        WHERE ud.user_id = s.user_id AND ud.is_active = TRUE \
    ), ARRAY[]::VARCHAR[]) AS designation_codes \
    FROM document_signers s JOIN users u ON u.user_id = s.user_id";

const SIGNER_DIRECTORY_SELECT: &str = "SELECT s.signer_id, s.user_id, s.name, \
    s.is_active, s.created_at, s.updated_at, s.updated_by, u.email AS user_email, \
    COALESCE(( \
        SELECT array_agg(d.code) FROM user_designations ud \
        JOIN designations d ON d.designation_id = ud.designation_id \
        WHERE ud.user_id = s.user_id AND ud.is_active = TRUE \
    ), ARRAY[]::VARCHAR[]) AS designation_codes \
    FROM document_signers s JOIN users u ON u.user_id = s.user_id";

impl DocumentManager {
    pub async fn list_signers(&self) -> Result<Vec<SignerDirectoryEntry>, String> {
        sqlx::query_as(&format!(
            "{} ORDER BY s.created_at DESC",
            SIGNER_DIRECTORY_SELECT
        ))
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn list_active_signers(&self) -> Result<Vec<SignerDirectoryEntry>, String> {
        sqlx::query_as(&format!(
            "{} WHERE s.is_active = TRUE ORDER BY s.name",
            SIGNER_DIRECTORY_SELECT
        ))
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Self-service: a user eligible to sign (holds `documents.signatures.sign`) fills
    /// in their own contact info from their Profile page. Upserts their one-and-only
    /// `document_signers` row (never creates one on behalf of another user).
    ///
    /// `name` is only used on first registration — it's what appears on signed
    /// documents, so it's fixed once set (not casually self-editable). Only `phone`
    /// and `email` can be updated afterward; if `name` is provided on an update it's
    /// silently ignored rather than erroring, so the caller doesn't need to branch.
    pub async fn upsert_own_signer_info(
        &self,
        user_id: i32,
        name: &str,
        phone: &str,
        email: &str,
    ) -> Result<Signer, String> {
        let existing = self.get_signer_by_user_id(user_id).await?;
        if existing.is_none() && name.trim().is_empty() {
            return Err("Name is required".to_string());
        }
        if phone.is_empty() || !phone.chars().all(|c| c.is_ascii_digit()) {
            return Err("Phone must contain digits only".to_string());
        }
        if !email.contains('@') {
            return Err("A valid email is required".to_string());
        }

        let signer_id: i32 = sqlx::query_scalar(
            "INSERT INTO document_signers (user_id, name, phone, email, updated_by) \
             VALUES ($1, $2, $3, $4, $1) \
             ON CONFLICT (user_id) DO UPDATE SET \
                 phone = EXCLUDED.phone, email = EXCLUDED.email, \
                 updated_at = NOW(), updated_by = EXCLUDED.updated_by \
             RETURNING signer_id",
        )
        .bind(user_id)
        .bind(name.trim())
        .bind(phone)
        .bind(email)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        self.get_signer(signer_id)
            .await?
            .ok_or_else(|| "Signer vanished after upsert".to_string())
    }

    pub async fn get_signer(&self, signer_id: i32) -> Result<Option<Signer>, String> {
        sqlx::query_as(&format!("{} WHERE s.signer_id = $1", SIGNER_SELECT))
            .bind(signer_id)
            .fetch_optional(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_signer_by_user_id(&self, user_id: i32) -> Result<Option<Signer>, String> {
        sqlx::query_as(&format!("{} WHERE s.user_id = $1", SIGNER_SELECT))
            .bind(user_id)
            .fetch_optional(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn set_signer_active(
        &self,
        signer_id: i32,
        is_active: bool,
        updated_by: i32,
    ) -> Result<(), String> {
        let result = sqlx::query(
            "UPDATE document_signers SET is_active = $1, updated_at = NOW(), updated_by = $2 \
             WHERE signer_id = $3",
        )
        .bind(is_active)
        .bind(updated_by)
        .bind(signer_id)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err("Signer not found".to_string());
        }
        Ok(())
    }

    /// Active designation codes held by each of the given signers' users — used to
    /// server-side verify a field's `required_signs_from_designations` ordering on attach.
    pub async fn designation_codes_for_signers(
        &self,
        signer_ids: &[i32],
    ) -> Result<std::collections::HashMap<i32, Vec<String>>, String> {
        if signer_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(i32, Vec<String>)> = sqlx::query_as(
            "SELECT s.signer_id, COALESCE(array_agg(d.code) FILTER (WHERE d.code IS NOT NULL), ARRAY[]::VARCHAR[]) \
             FROM document_signers s \
             LEFT JOIN user_designations ud ON ud.user_id = s.user_id AND ud.is_active = TRUE \
             LEFT JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE s.signer_id = ANY($1) \
             GROUP BY s.signer_id",
        )
        .bind(signer_ids)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().collect())
    }
}
