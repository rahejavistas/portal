//! DB-backed, versioned process templates. Every save appends a new
//! process_template_versions row rather than mutating one in place; a process
//! instance is permanently pinned to one version_id for its whole lifetime.
//! step_id/field_id only need to be unique within one version_id.

use serde::Deserialize;
use sqlx::Connection;

#[derive(Debug, Clone, Deserialize)]
pub struct FieldDraft {
    pub name: String,
    pub r#type: String,
    #[serde(default)]
    pub is_optional: bool,
    pub filled_by: String,
    #[serde(default)]
    pub requirements: Option<serde_json::Value>,
    #[serde(default)]
    pub requires_signature: bool,
    #[serde(default)]
    pub required_signs_from_designations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepDraft {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(default)]
    pub is_review_required: bool,
    #[serde(default)]
    pub fields: Option<Vec<FieldDraft>>,
    #[serde(default)]
    pub substeps: Option<Vec<StepDraft>>,
}

/// The whole tree posted by the builder UI on Save. Deliberately carries no IDs at
/// all — the server always allocates fresh step_id/field_id values.
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateDraft {
    /// `None` (or an unknown id) creates a brand-new template; an existing
    /// `template_id` appends a new draft version to it.
    #[serde(default)]
    pub template_id: Option<i32>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(default)]
    pub is_review_required: bool,
    #[serde(default)]
    pub fields: Option<Vec<FieldDraft>>,
    #[serde(default)]
    pub substeps: Option<Vec<StepDraft>>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct TemplateVersionSummary {
    pub version_id: i32,
    pub version_number: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<i32>,
    pub created_by_email: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TemplateSummary {
    pub template_id: i32,
    pub name: String,
    pub active_version_id: Option<i32>,
    pub versions: Vec<TemplateVersionSummary>,
}

fn assign_field_ids(
    drafts: Option<Vec<FieldDraft>>,
    next_id: &mut i32,
) -> Option<Vec<crate::ProcessFieldTemplate>> {
    drafts.map(|fields| {
        fields
            .into_iter()
            .map(|f| {
                let field_id = *next_id;
                *next_id += 1;
                crate::ProcessFieldTemplate {
                    field_id,
                    name: f.name,
                    r#type: f.r#type,
                    is_optional: f.is_optional,
                    filled_by: f.filled_by,
                    requirements: f.requirements,
                    requires_signature: f.requires_signature,
                    required_signs_from_designations: f.required_signs_from_designations,
                }
            })
            .collect()
    })
}

fn assign_step_ids(draft: StepDraft, next_id: &mut i32) -> crate::ProcessStepTemplate {
    let step_id = *next_id;
    *next_id += 1;
    let fields = assign_field_ids(draft.fields, next_id);
    let substeps = draft.substeps.map(|subs| {
        subs.into_iter()
            .map(|s| assign_step_ids(s, next_id))
            .collect()
    });
    crate::ProcessStepTemplate {
        step_id,
        name: draft.name,
        description: draft.description,
        is_optional: draft.is_optional,
        is_review_required: draft.is_review_required,
        fields,
        substeps,
    }
}

/// Loads one pinned version by its version_id.
pub async fn load_template_version(
    conn: &mut sqlx::PgConnection,
    version_id: i32,
) -> Result<crate::ProcessTemplate, String> {
    get_template_version_detail(conn, version_id)
        .await?
        .ok_or_else(|| format!("Unknown template version_id: {}", version_id))
}

pub async fn get_template_version_detail(
    conn: &mut sqlx::PgConnection,
    version_id: i32,
) -> Result<Option<crate::ProcessTemplate>, String> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT definition FROM process_template_versions WHERE version_id = $1")
            .bind(version_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;

    match row {
        Some((def,)) => Ok(Some(
            serde_json::from_value(def).map_err(|e| e.to_string())?,
        )),
        None => Ok(None),
    }
}

/// The version_id residents should start new processes against right now, if any.
pub async fn get_active_version_id(
    conn: &mut sqlx::PgConnection,
    template_id: i32,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<i32>>(
        "SELECT active_version_id FROM process_templates WHERE template_id = $1",
    )
    .bind(template_id)
    .fetch_optional(&mut *conn)
    .await
    .map(|opt| opt.flatten())
}

/// `(template_id, name)` for every template that currently has an active version —
/// what a resident is allowed to start.
pub async fn list_startable_templates(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<(i32, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT template_id, name FROM process_templates \
         WHERE active_version_id IS NOT NULL ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await
}

/// Full admin listing: every template with its active version and version history.
pub async fn list_templates(conn: &mut sqlx::PgConnection) -> Result<Vec<TemplateSummary>, String> {
    let templates: Vec<(i32, String, Option<i32>)> = sqlx::query_as(
        "SELECT template_id, name, active_version_id FROM process_templates ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(templates.len());
    for (template_id, name, active_version_id) in templates {
        let versions: Vec<TemplateVersionSummary> = sqlx::query_as(
            "SELECT ptv.version_id, ptv.version_number, ptv.created_at, ptv.created_by, u.email AS created_by_email \
             FROM process_template_versions ptv \
             LEFT JOIN users u ON u.user_id = ptv.created_by \
             WHERE ptv.template_id = $1 ORDER BY ptv.version_number DESC",
        )
        .bind(template_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

        out.push(TemplateSummary {
            template_id,
            name,
            active_version_id,
            versions,
        });
    }
    Ok(out)
}

/// Upsert: creates a new template, or appends a new draft version (never
/// auto-activates). Returns the new version_id.
pub async fn save_template_version(
    conn: &mut sqlx::PgConnection,
    draft: TemplateDraft,
    created_by: i32,
) -> Result<i32, String> {
    let mut tx = conn.begin().await.map_err(|e| e.to_string())?;

    let (template_id, new_version): (i32, i32) = match draft.template_id {
        Some(id) => {
            let exists: Option<(i32,)> =
                sqlx::query_as("SELECT template_id FROM process_templates WHERE template_id = $1")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            exists.ok_or_else(|| format!("Unknown template_id: {}", id))?;
            let max_version: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(version_number) FROM process_template_versions WHERE template_id = $1",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            (id, max_version.unwrap_or(0) + 1)
        }
        None => {
            let id: i32 = sqlx::query_scalar(
                "INSERT INTO process_templates (name) VALUES ($1) RETURNING template_id",
            )
            .bind(&draft.name)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("Could not create template: {}", e))?;
            (id, 1)
        }
    };

    let name = draft.name.clone();
    let description = draft.description.clone();
    let is_optional = draft.is_optional;
    let is_review_required = draft.is_review_required;
    let fields_draft = draft.fields;
    let substeps_draft = draft.substeps;

    let mut next_id = 1;
    let root_step_id = next_id;
    next_id += 1;
    let fields = assign_field_ids(fields_draft, &mut next_id);
    let substeps = substeps_draft.map(|subs| {
        subs.into_iter()
            .map(|s| assign_step_ids(s, &mut next_id))
            .collect()
    });

    let template = crate::ProcessTemplate {
        name: name.clone(),
        description,
        step_id: root_step_id,
        is_optional,
        is_review_required,
        substeps,
        fields,
    };

    let definition = serde_json::to_value(&template).map_err(|e| e.to_string())?;

    let version_id: i32 = sqlx::query_scalar(
        "INSERT INTO process_template_versions (template_id, version_number, definition, created_by) \
         VALUES ($1, $2, $3, $4) RETURNING version_id",
    )
    .bind(template_id)
    .bind(new_version)
    .bind(&definition)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE process_templates SET name = $1, updated_at = NOW() WHERE template_id = $2",
    )
    .bind(&name)
    .bind(template_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    Ok(version_id)
}

pub async fn activate_template_version(
    conn: &mut sqlx::PgConnection,
    template_id: i32,
    version_id: i32,
) -> Result<(), String> {
    let exists: Option<(i32,)> = sqlx::query_as(
        "SELECT version_id FROM process_template_versions \
         WHERE version_id = $1 AND template_id = $2",
    )
    .bind(version_id)
    .bind(template_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;

    if exists.is_none() {
        return Err(format!(
            "version_id {} does not belong to template {}",
            version_id, template_id
        ));
    }

    sqlx::query(
        "UPDATE process_templates SET active_version_id = $1, updated_at = NOW() \
         WHERE template_id = $2",
    )
    .bind(version_id)
    .bind(template_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
