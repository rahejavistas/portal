//! Document category CRUD and the category↔designation access matrix
//! (`document_categories` / `document_category_designations`).

use super::{CategoryDesignation, CategoryWithSubcategories, DocumentCategoryDef};
use crate::document::DocumentManager;

impl DocumentManager {
    /// Every category `user` can manage, in one query. `None` means all categories.
    pub async fn manageable_categories(
        &self,
        user: &authentication::User,
    ) -> Result<Option<std::collections::HashSet<String>>, String> {
        if user.has_permission(authentication::Permission::DocumentsAllManage) {
            return Ok(None);
        }
        let categories: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT dcd.category FROM document_category_designations dcd \
             JOIN user_designations ud ON ud.designation_id = dcd.designation_id \
             WHERE dcd.can_manage = TRUE AND ud.user_id = $1 AND ud.is_active = TRUE",
        )
        .bind(user.user_id)
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(Some(categories.into_iter().collect()))
    }

    /// Lists all document categories (including the two protected system ones),
    /// each with its subcategories nested in, so callers get both in one round trip.
    pub async fn list_document_categories(&self) -> Result<Vec<CategoryWithSubcategories>, String> {
        let categories: Vec<DocumentCategoryDef> = sqlx::query_as(
            "SELECT code, label, is_public_read, is_flat_scoped, is_system \
             FROM document_categories ORDER BY is_system DESC, code",
        )
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let all_subcategories = self.list_document_subcategories(None).await?;

        Ok(categories
            .into_iter()
            .map(|category| {
                let subcategories = all_subcategories
                    .iter()
                    .filter(|s| s.category == category.code)
                    .cloned()
                    .collect();
                CategoryWithSubcategories {
                    category,
                    subcategories,
                }
            })
            .collect())
    }

    /// Renames a category's display label. The code (and every document/permission
    /// reference keyed on it) is untouched.
    pub async fn rename_document_category(
        &self,
        code: &str,
        new_label: &str,
    ) -> Result<(), String> {
        let result = sqlx::query("UPDATE document_categories SET label = $1 WHERE code = $2")
            .bind(new_label)
            .bind(code)
            .execute(&self.pg_pool)
            .await
            .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err(format!("Unknown document category: {}", code));
        }
        Ok(())
    }

    /// Creates a new, non-system document category. PUBLIC/RESIDENT_SPECIFIC are seed-only.
    pub async fn create_document_category(
        &self,
        code: &str,
        label: &str,
        created_by: Option<i32>,
    ) -> Result<DocumentCategoryDef, String> {
        let result: DocumentCategoryDef = sqlx::query_as(
            "INSERT INTO document_categories (code, label, is_public_read, is_flat_scoped, is_system, created_by) \
             VALUES ($1, $2, FALSE, FALSE, FALSE, $3) \
             RETURNING code, label, is_public_read, is_flat_scoped, is_system",
        )
        .bind(code)
        .bind(label)
        .bind(created_by)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let _ = authentication::log_permission_change(
            &self.pg_pool,
            format!("Created document category '{}'", code),
            created_by,
        )
        .await;
        Ok(result)
    }

    /// Deletes a non-system document category. Fails if any document still uses it or if
    /// it's a protected system category (PUBLIC/RESIDENT_SPECIFIC).
    pub async fn delete_document_category(
        &self,
        code: &str,
        deleted_by: Option<i32>,
    ) -> Result<(), String> {
        let is_system: Option<bool> =
            sqlx::query_scalar("SELECT is_system FROM document_categories WHERE code = $1")
                .bind(code)
                .fetch_optional(&self.pg_pool)
                .await
                .map_err(|e| e.to_string())?;

        match is_system {
            None => return Err(format!("Unknown document category: {}", code)),
            Some(true) => {
                return Err(format!(
                    "'{}' is a system category and cannot be deleted.",
                    code
                ))
            }
            Some(false) => {}
        }

        sqlx::query("DELETE FROM document_categories WHERE code = $1")
            .bind(code)
            .execute(&self.pg_pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("foreign key") {
                    "Cannot delete: documents still exist in this category.".to_string()
                } else {
                    e.to_string()
                }
            })?;

        let _ = authentication::log_permission_change(
            &self.pg_pool,
            format!("Deleted document category '{}'", code),
            deleted_by,
        )
        .await;
        Ok(())
    }

    /// Lists the full document-category-to-designation access matrix.
    pub async fn list_category_designations(&self) -> Result<Vec<CategoryDesignation>, String> {
        sqlx::query_as(
            "SELECT dcd.category, d.code AS designation_code, dcd.can_read, dcd.can_manage \
             FROM document_category_designations dcd \
             JOIN designations d ON d.designation_id = dcd.designation_id \
             WHERE d.is_active = TRUE \
             ORDER BY dcd.category, d.code",
        )
        .fetch_all(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Sets a designation's read/manage access to a document category (upsert).
    pub async fn set_category_designation(
        &self,
        category: &str,
        designation_code: &str,
        can_read: bool,
        can_manage: bool,
        updated_by: Option<i32>,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO document_category_designations (category, designation_id, can_read, can_manage, updated_by) \
             SELECT $1, designation_id, $3, $4, $5 FROM designations WHERE code = $2 \
             ON CONFLICT (category, designation_id) \
             DO UPDATE SET can_read = EXCLUDED.can_read, can_manage = EXCLUDED.can_manage, \
                 updated_by = EXCLUDED.updated_by, updated_at = NOW()",
        )
        .bind(category)
        .bind(designation_code)
        .bind(can_read)
        .bind(can_manage)
        .bind(updated_by)
        .execute(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;

        let _ = authentication::log_permission_change(
            &self.pg_pool,
            format!(
                "Set '{}' access on category '{}': read={}, manage={}",
                designation_code, category, can_read, can_manage
            ),
            updated_by,
        )
        .await;
        Ok(())
    }
}
