//! DB-backed, versioned process templates. Every save appends a new
//! process_template_versions row rather than mutating one in place; a process
//! instance is permanently pinned to one version_id for its whole lifetime.
//! step_id/field_id only need to be unique within one version_id.

use serde::Deserialize;
use sqlx::Connection;

#[derive(Debug)]
pub enum TemplateStoreError {
    /// A referenced template/version id doesn't exist — safe to show the caller.
    NotFound(String),
    /// A field references a document it isn't allowed to (e.g. a non-PUBLIC example document).
    InvalidReference(String),
    Db(sqlx::Error),
    Serialize(serde_json::Error),
}

impl std::fmt::Display for TemplateStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateStoreError::NotFound(msg) => write!(f, "{}", msg),
            TemplateStoreError::InvalidReference(msg) => write!(f, "{}", msg),
            TemplateStoreError::Db(e) => write!(f, "{}", e),
            TemplateStoreError::Serialize(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for TemplateStoreError {
    fn from(e: sqlx::Error) -> Self {
        TemplateStoreError::Db(e)
    }
}

impl From<serde_json::Error> for TemplateStoreError {
    fn from(e: serde_json::Error) -> Self {
        TemplateStoreError::Serialize(e)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldDraft {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub r#type: crate::FieldType,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(default)]
    pub requirements: Option<serde_json::Value>,
    #[serde(default)]
    pub requires_signature: bool,
    #[serde(default)]
    pub required_signs_from_designations: Vec<String>,
    /// `Enum` fields only — the closed list of valid values.
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub example_document_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepDraft {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_optional: bool,
    /// Who fills this stage. Only meaningful on a top-level Stage draft — the builder
    /// doesn't expose the picker on a Substep, so this is `None` there and inherited from
    /// the enclosing Stage when the tree is assembled.
    #[serde(default)]
    pub filled_by: Option<crate::StepOwner>,
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
    pub fields: Option<Vec<FieldDraft>>,
    #[serde(default)]
    pub substeps: Option<Vec<StepDraft>>,
    /// `None` leaves SLA tracking off for this template.
    #[serde(default)]
    pub tat_working_days: Option<i32>,
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
                    description: f.description,
                    r#type: f.r#type,
                    is_optional: f.is_optional,
                    requirements: f.requirements,
                    requires_signature: f.requires_signature,
                    required_signs_from_designations: f.required_signs_from_designations,
                    options: f.options,
                    example_document_id: f.example_document_id,
                }
            })
            .collect()
    })
}

fn collect_example_document_ids(
    fields: &Option<Vec<crate::ProcessFieldTemplate>>,
    substeps: &Option<Vec<crate::ProcessStepTemplate>>,
    out: &mut Vec<uuid::Uuid>,
) {
    if let Some(fs) = fields {
        out.extend(fs.iter().filter_map(|f| f.example_document_id));
    }
    if let Some(subs) = substeps {
        for s in subs {
            collect_example_document_ids(&s.fields, &s.substeps, out);
        }
    }
}

fn collect_designation_codes(
    substeps: &Option<Vec<crate::ProcessStepTemplate>>,
    out: &mut Vec<String>,
) {
    let Some(subs) = substeps else { return };
    for s in subs {
        if let crate::StepOwner::Designation { code } = &s.filled_by {
            out.push(code.clone());
        }
        collect_designation_codes(&s.substeps, out);
    }
}

/// Denormalizes owner onto every substep so downstream code never walks back up a parent.
fn assign_step_ids(
    draft: StepDraft,
    next_id: &mut i32,
    inherited_owner: crate::StepOwner,
) -> crate::ProcessStepTemplate {
    let step_id = *next_id;
    *next_id += 1;
    let owner = draft.filled_by.unwrap_or(inherited_owner);
    let fields = assign_field_ids(draft.fields, next_id);
    let substeps = draft.substeps.map(|subs| {
        subs.into_iter()
            .map(|s| assign_step_ids(s, next_id, owner.clone()))
            .collect()
    });
    crate::ProcessStepTemplate {
        step_id,
        name: draft.name,
        description: draft.description,
        is_optional: draft.is_optional,
        filled_by: owner,
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
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Unknown template version_id: {}", version_id))
}

pub async fn get_template_version_detail(
    conn: &mut sqlx::PgConnection,
    version_id: i32,
) -> Result<Option<crate::ProcessTemplate>, TemplateStoreError> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT definition FROM process_template_versions WHERE version_id = $1")
            .bind(version_id)
            .fetch_optional(&mut *conn)
            .await?;

    match row {
        Some((def,)) => Ok(Some(serde_json::from_value(def)?)),
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

/// Outcome of a successful `save_template_version` call.
pub struct SaveTemplateOutcome {
    pub version_id: i32,
    /// Distinct designation codes referenced anywhere in the saved tree — the caller
    /// grants these `process.view`/`process.reviewer` (this crate has no dependency on
    /// `authentication`, so it can't do that itself).
    pub designation_codes: Vec<String>,
}

/// Upsert: creates a new template, or appends a new draft version (never auto-activates).
pub async fn save_template_version(
    conn: &mut sqlx::PgConnection,
    draft: TemplateDraft,
    created_by: i32,
) -> Result<SaveTemplateOutcome, TemplateStoreError> {
    let mut tx = conn.begin().await?;

    let (template_id, new_version): (i32, i32) = match draft.template_id {
        Some(id) => {
            let exists: Option<(i32,)> =
                sqlx::query_as("SELECT template_id FROM process_templates WHERE template_id = $1")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?;
            exists.ok_or_else(|| {
                TemplateStoreError::NotFound(format!("Unknown template_id: {}", id))
            })?;
            let max_version: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(version_number) FROM process_template_versions WHERE template_id = $1",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            (id, max_version.unwrap_or(0) + 1)
        }
        None => {
            let id: i32 = sqlx::query_scalar(
                "INSERT INTO process_templates (name) VALUES ($1) RETURNING template_id",
            )
            .bind(&draft.name)
            .fetch_one(&mut *tx)
            .await?;
            (id, 1)
        }
    };

    let name = draft.name.clone();
    let description = draft.description.clone();
    let is_optional = draft.is_optional;
    let tat_working_days = draft.tat_working_days;
    let fields_draft = draft.fields;
    let substeps_draft = draft.substeps;

    let mut next_id = 1;
    let root_step_id = next_id;
    next_id += 1;
    let fields = assign_field_ids(fields_draft, &mut next_id);
    let substeps = substeps_draft.map(|subs| {
        subs.into_iter()
            .map(|s| assign_step_ids(s, &mut next_id, crate::StepOwner::User))
            .collect()
    });

    let template = crate::ProcessTemplate {
        name: name.clone(),
        description,
        step_id: root_step_id,
        is_optional,
        substeps,
        fields,
        tat_working_days,
        filled_by: crate::StepOwner::User,
    };

    let mut example_doc_ids = Vec::new();
    collect_example_document_ids(&template.fields, &template.substeps, &mut example_doc_ids);
    if !example_doc_ids.is_empty() {
        example_doc_ids.sort();
        example_doc_ids.dedup();
        let public_ids: Vec<uuid::Uuid> = sqlx::query_scalar(
            "SELECT document_id FROM documents WHERE document_id = ANY($1) AND category = 'PUBLIC' AND NOT is_deleted",
        )
        .bind(&example_doc_ids)
        .fetch_all(&mut *tx)
        .await?;
        let public_set: std::collections::HashSet<_> = public_ids.into_iter().collect();
        if example_doc_ids.iter().any(|id| !public_set.contains(id)) {
            return Err(TemplateStoreError::InvalidReference(
                "Example document must be an existing PUBLIC document.".to_string(),
            ));
        }
    }

    let definition = serde_json::to_value(&template)?;

    let version_id: i32 = sqlx::query_scalar(
        "INSERT INTO process_template_versions (template_id, version_number, definition, created_by) \
         VALUES ($1, $2, $3, $4) RETURNING version_id",
    )
    .bind(template_id)
    .bind(new_version)
    .bind(&definition)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE process_templates SET name = $1, updated_at = NOW() WHERE template_id = $2",
    )
    .bind(&name)
    .bind(template_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let mut designation_codes = Vec::new();
    collect_designation_codes(&template.substeps, &mut designation_codes);
    designation_codes.sort();
    designation_codes.dedup();

    Ok(SaveTemplateOutcome {
        version_id,
        designation_codes,
    })
}

pub async fn activate_template_version(
    conn: &mut sqlx::PgConnection,
    template_id: i32,
    version_id: i32,
) -> Result<(), TemplateStoreError> {
    let exists: Option<(i32,)> = sqlx::query_as(
        "SELECT version_id FROM process_template_versions \
         WHERE version_id = $1 AND template_id = $2",
    )
    .bind(version_id)
    .bind(template_id)
    .fetch_optional(&mut *conn)
    .await?;

    if exists.is_none() {
        return Err(TemplateStoreError::NotFound(format!(
            "version_id {} does not belong to template {}",
            version_id, template_id
        )));
    }

    sqlx::query(
        "UPDATE process_templates SET active_version_id = $1, updated_at = NOW() \
         WHERE template_id = $2",
    )
    .bind(version_id)
    .bind(template_id)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Clears `active_version_id` — no resident can start a new process against this
/// template until it's activated again. In-progress processes stay pinned to their
/// own version and are unaffected.
pub async fn deactivate_template(
    conn: &mut sqlx::PgConnection,
    template_id: i32,
) -> Result<(), String> {
    let updated = sqlx::query(
        "UPDATE process_templates SET active_version_id = NULL, updated_at = NOW() \
         WHERE template_id = $1",
    )
    .bind(template_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;

    if updated.rows_affected() == 0 {
        return Err(format!("Unknown template_id: {}", template_id));
    }
    Ok(())
}
