//! Document subcategory CRUD — optional, UI-only tag scoped to a parent category.
//! Never participates in access control, unlike categories.

use super::DocumentSubcategoryDef;
use crate::document::DocumentManager;

impl DocumentManager {
    /// `None` lists all subcategories; `Some(category)` scopes to one.
    pub async fn list_document_subcategories(
        &self,
        category: Option<&str>,
    ) -> Result<Vec<DocumentSubcategoryDef>, String> {
        match category {
            Some(cat) => sqlx::query_as(
                "SELECT subcategory_id, category, code, label FROM document_subcategories \
                 WHERE category = $1 ORDER BY label",
            )
            .bind(cat)
            .fetch_all(&self.pg_pool)
            .await
            .map_err(|e| e.to_string()),
            None => sqlx::query_as(
                "SELECT subcategory_id, category, code, label FROM document_subcategories \
                 ORDER BY category, label",
            )
            .fetch_all(&self.pg_pool)
            .await
            .map_err(|e| e.to_string()),
        }
    }

    pub async fn resolve_subcategory_id(&self, category: &str, code: &str) -> Result<i32, String> {
        let id: Option<i32> = sqlx::query_scalar(
            "SELECT subcategory_id FROM document_subcategories WHERE category = $1 AND code = $2",
        )
        .bind(category)
        .bind(code)
        .fetch_optional(&self.pg_pool)
        .await
        .map_err(|e| e.to_string())?;
        id.ok_or_else(|| format!("Unknown subcategory '{}' for category '{}'", code, category))
    }

    pub async fn create_document_subcategory(
        &self,
        category: &str,
        code: &str,
        label: &str,
        created_by: Option<i32>,
    ) -> Result<DocumentSubcategoryDef, String> {
        let result: DocumentSubcategoryDef = sqlx::query_as(
            "INSERT INTO document_subcategories (category, code, label, created_by) \
             VALUES ($1, $2, $3, $4) \
             RETURNING subcategory_id, category, code, label",
        )
        .bind(category)
        .bind(code)
        .bind(label)
        .bind(created_by)
        .fetch_one(&self.pg_pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("foreign key") {
                format!("Unknown document category: {}", category)
            } else if msg.contains("unique") {
                format!(
                    "Subcategory '{}' already exists under category '{}'.",
                    code, category
                )
            } else {
                msg
            }
        })?;

        let _ = authentication::log_permission_change(
            &self.pg_pool,
            format!(
                "Created document subcategory '{}' under category '{}'",
                code, category
            ),
            created_by,
        )
        .await;
        Ok(result)
    }

    pub async fn rename_document_subcategory(
        &self,
        subcategory_id: i32,
        new_label: &str,
    ) -> Result<(), String> {
        let result =
            sqlx::query("UPDATE document_subcategories SET label = $1 WHERE subcategory_id = $2")
                .bind(new_label)
                .bind(subcategory_id)
                .execute(&self.pg_pool)
                .await
                .map_err(|e| e.to_string())?;
        if result.rows_affected() == 0 {
            return Err(format!("Unknown subcategory ID: {}", subcategory_id));
        }
        Ok(())
    }
}
