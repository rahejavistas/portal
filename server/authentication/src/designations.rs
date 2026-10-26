use crate::{log_permission_change, PermissionsManager};

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Designation {
    pub designation_id: i32,
    pub code: String,
    pub label: String,
    pub is_system: bool,
    pub permission_codes: Vec<String>,
}

/// Raw `permissions` row, for listing/display. `Permission` (in `lib.rs`) is for checks.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PermissionRow {
    pub permission_id: i32,
    pub code: String,
    pub label: String,
}

impl PermissionsManager {
    pub async fn list_designations(&self) -> Result<Vec<Designation>, sqlx::Error> {
        sqlx::query_as(
            "SELECT d.designation_id, d.code, d.label, d.is_system, \
                    COALESCE(ARRAY_AGG(p.code::TEXT) FILTER (WHERE p.code IS NOT NULL), ARRAY[]::TEXT[]) AS permission_codes \
             FROM designations d \
             LEFT JOIN designation_permissions dp ON dp.designation_id = d.designation_id \
             LEFT JOIN permissions p ON p.permission_id = dp.permission_id \
             WHERE d.is_active = TRUE \
             GROUP BY d.designation_id \
             ORDER BY d.designation_id",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Whether a designation code is a protected system designation. `None` if unknown
    /// or soft-deleted — deleted designations are treated as nonexistent everywhere.
    pub async fn is_system_designation(&self, code: &str) -> Result<Option<bool>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT is_system FROM designations WHERE code = $1 AND is_active = TRUE",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_permissions(&self) -> Result<Vec<PermissionRow>, sqlx::Error> {
        sqlx::query_as("SELECT permission_id, code, label FROM permissions ORDER BY permission_id")
            .fetch_all(&self.pool)
            .await
    }

    /// Soft-deletes a non-system designation (sets `is_active = FALSE` — the row, and its
    /// `designation_permissions`/`permission_log` history, are kept for audit purposes).
    /// Fails if it's still assigned to any user.
    pub async fn delete_designation(
        &self,
        code: &str,
        deleted_by: Option<i32>,
    ) -> Result<(), String> {
        // Not `is_system` (its permissions are meant to be editable), but
        // `create_staff_user` hardcodes this code — deleting it would break staff
        // account creation.
        if code == "staff" {
            return Err("'staff' cannot be deleted.".to_string());
        }
        self.require_editable_designation(code).await?;

        let still_assigned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_designations ud \
             JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE d.code = $1 AND ud.is_active = TRUE)",
        )
        .bind(code)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        if still_assigned {
            return Err(
                "Cannot delete: this designation is still assigned to one or more users."
                    .to_string(),
            );
        }

        let is_wing_representative: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM wings w \
             JOIN designations d ON d.designation_id = w.representative_designation_id \
             WHERE d.code = $1)",
        )
        .bind(code)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if is_wing_representative {
            return Err("Cannot delete: this designation is a wing's representative.".to_string());
        }

        sqlx::query("UPDATE designations SET is_active = FALSE WHERE code = $1")
            .bind(code)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;

        let _ = log_permission_change(
            &self.pool,
            format!("Deleted designation '{}'", code),
            deleted_by,
        )
        .await;
        Ok(())
    }

    /// Errors if `code` is unknown or a protected system designation.
    async fn require_editable_designation(&self, code: &str) -> Result<(), String> {
        match self
            .is_system_designation(code)
            .await
            .map_err(|e| e.to_string())?
        {
            None => Err(format!("Unknown designation: {}", code)),
            Some(true) => Err(format!(
                "'{}' is a system designation and cannot be edited.",
                code
            )),
            Some(false) => Ok(()),
        }
    }

    /// Renames a designation's label; the code (and everything keyed on it) is untouched.
    pub async fn rename_designation(&self, code: &str, new_label: &str) -> Result<(), String> {
        let result = sqlx::query("UPDATE designations SET label = $1 WHERE code = $2")
            .bind(new_label)
            .bind(code)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err(format!("Unknown designation: {}", code));
        }
        Ok(())
    }

    pub async fn create_designation(
        &self,
        code: &str,
        label: &str,
        created_by: Option<i32>,
    ) -> Result<Designation, String> {
        if code.starts_with("wing_rep_") {
            return Err("Designation codes starting with 'wing_rep_' are reserved.".to_string());
        }

        let designation = sqlx::query_as(
            "INSERT INTO designations (code, label, is_system, created_by) VALUES ($1, $2, FALSE, $3) \
             RETURNING designation_id, code, label, is_system, ARRAY[]::TEXT[] AS permission_codes",
        )
        .bind(code)
        .bind(label)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let _ = log_permission_change(
            &self.pool,
            format!("Created designation '{}'", code),
            created_by,
        )
        .await;
        Ok(designation)
    }

    /// Idempotent: a flat's wing must have a row here (and a representative designation)
    /// before the flat itself can be inserted, since `flats.wing` references `wings(name)`.
    pub async fn ensure_wing_exists(
        &self,
        wing: &str,
        created_by: Option<i32>,
    ) -> Result<(), String> {
        let exists: Option<(String,)> = sqlx::query_as("SELECT name FROM wings WHERE name = $1")
            .bind(wing)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        if exists.is_some() {
            return Ok(());
        }

        let code = wing_representative_designation_code(wing);
        let label = format!("Wing Representative {}", wing);
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        let designation_id: i32 = sqlx::query_scalar(
            "INSERT INTO designations (code, label, is_system, created_by) VALUES ($1, $2, FALSE, $3) \
             ON CONFLICT (code) DO UPDATE SET code = EXCLUDED.code \
             RETURNING designation_id",
        )
        .bind(&code)
        .bind(&label)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO wings (name, representative_designation_id) VALUES ($1, $2) \
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(wing)
        .bind(designation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Grants or revokes a single permission for a designation.
    pub async fn toggle_designation_permission(
        &self,
        designation_code: &str,
        permission_code: &str,
        enabled: bool,
        actor: Option<i32>,
    ) -> Result<(), String> {
        if enabled {
            sqlx::query(
                "INSERT INTO designation_permissions (designation_id, permission_id, granted_by) \
                 SELECT d.designation_id, p.permission_id, $3 FROM designations d, permissions p \
                 WHERE d.code = $1 AND p.code = $2 \
                 ON CONFLICT (designation_id, permission_id) \
                 DO UPDATE SET granted_by = EXCLUDED.granted_by, granted_at = NOW()",
            )
            .bind(designation_code)
            .bind(permission_code)
            .bind(actor)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        } else {
            sqlx::query(
                "DELETE FROM designation_permissions \
                 WHERE designation_id = (SELECT designation_id FROM designations WHERE code = $1) \
                   AND permission_id = (SELECT permission_id FROM permissions WHERE code = $2)",
            )
            .bind(designation_code)
            .bind(permission_code)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }

        let verb = if enabled { "Granted" } else { "Revoked" };
        let _ = log_permission_change(
            &self.pool,
            format!(
                "{} permission '{}' {} designation '{}'",
                verb,
                permission_code,
                if enabled { "to" } else { "from" },
                designation_code
            ),
            actor,
        )
        .await;
        Ok(())
    }

    pub async fn get_designation_permissions(
        &self,
        designation_code: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT p.code FROM designation_permissions dp \
             JOIN permissions p ON p.permission_id = dp.permission_id \
             JOIN designations d ON d.designation_id = dp.designation_id \
             WHERE d.code = $1",
        )
        .bind(designation_code)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_user_designations(&self, user_id: i32) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT d.code FROM user_designations ud \
             JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn user_has_permission(
        &self,
        user_id: i32,
        permission_code: &str,
    ) -> Result<bool, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_designations ud \
             JOIN designation_permissions dp ON dp.designation_id = ud.designation_id \
             JOIN permissions p ON p.permission_id = dp.permission_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE AND p.code = $2",
        )
        .bind(user_id)
        .bind(permission_code)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    /// Emails of every user holding the given permission via an active designation.
    /// Used to notify a dynamic recipient list (e.g. everyone who can sign documents)
    /// rather than a single fixed address.
    pub async fn emails_with_permission(
        &self,
        permission_code: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT DISTINCT u.email FROM users u \
             JOIN user_designations ud ON ud.user_id = u.user_id AND ud.is_active = TRUE \
             JOIN designation_permissions dp ON dp.designation_id = ud.designation_id \
             JOIN permissions p ON p.permission_id = dp.permission_id \
             WHERE p.code = $1 AND u.is_active = TRUE",
        )
        .bind(permission_code)
        .fetch_all(&self.pool)
        .await
    }

    /// Codes of every designation this user actively holds. Always queried live.
    pub async fn active_designation_codes(
        &self,
        user_id: i32,
    ) -> Result<std::collections::HashSet<String>, sqlx::Error> {
        let codes: Vec<String> = sqlx::query_scalar(
            "SELECT d.code FROM designations d \
             JOIN user_designations ud ON ud.designation_id = d.designation_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(codes.into_iter().collect())
    }

    /// Emails of every active holder of the given designation.
    pub async fn emails_for_designation(
        &self,
        designation_code: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT DISTINCT u.email FROM users u \
             JOIN user_designations ud ON ud.user_id = u.user_id AND ud.is_active = TRUE \
             JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE d.code = $1 AND u.is_active = TRUE",
        )
        .bind(designation_code)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_designation_label(&self, code: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT label FROM designations WHERE code = $1")
            .bind(code)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn assign_designation(
        &self,
        user_id: i32,
        designation_code: &str,
        assigned_by: Option<i32>,
    ) -> Result<(), String> {
        // DO UPDATE (not DO NOTHING) so RETURNING always yields a row on success.
        let assigned: Option<(i32,)> = sqlx::query_as(
            "WITH target AS ( \
                 SELECT designation_id FROM designations WHERE code = $1 AND is_active = TRUE \
             ) \
             INSERT INTO user_designations (user_id, designation_id, assigned_by) \
             SELECT $2, designation_id, $3 FROM target \
             ON CONFLICT (user_id, designation_id) WHERE is_active = TRUE \
             DO UPDATE SET assigned_by = EXCLUDED.assigned_by, updated_at = NOW() \
             RETURNING designation_id",
        )
        .bind(designation_code)
        .bind(user_id)
        .bind(assigned_by)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        if assigned.is_none() {
            return Err(format!("Unknown designation: {}", designation_code));
        }

        let _ = log_permission_change(
            &self.pool,
            format!(
                "Assigned designation '{}' to user #{}",
                designation_code, user_id
            ),
            assigned_by,
        )
        .await;
        Ok(())
    }

    /// A no-op revoke (never held, or already revoked) is a success, not an error —
    /// callers can always retry a revoke safely. Returns whether a row actually
    /// changed, so callers only audit-log/notify on a real change.
    pub async fn revoke_designation(
        &self,
        user_id: i32,
        designation_code: &str,
        revoked_by: Option<i32>,
    ) -> Result<bool, String> {
        let result = sqlx::query(
            "UPDATE user_designations SET is_active = FALSE, updated_at = NOW() \
             WHERE user_id = $1 AND is_active = TRUE \
               AND designation_id = (SELECT designation_id FROM designations WHERE code = $2)",
        )
        .bind(user_id)
        .bind(designation_code)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let changed = result.rows_affected() > 0;
        if changed {
            let _ = log_permission_change(
                &self.pool,
                format!(
                    "Revoked designation '{}' from user #{}",
                    designation_code, user_id
                ),
                revoked_by,
            )
            .await;
        }
        Ok(changed)
    }
}

/// Cosmetic only: naming for a newly auto-created designation. Resolution elsewhere
/// always goes through `wings.representative_designation_id`, never recomputes this.
fn wing_representative_designation_code(wing: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for c in wing.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    format!("wing_rep_{}", slug)
}
