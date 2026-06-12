use crate::document::{Document, DocumentAccessLevel};
use crate::extractors::SessionUser;
use crate::state::AppState;
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Json,
};
use process::db::{
    find_process_by_ownership_and_template, get_active_ownerships_for_user, get_field_history,
    get_process_info, get_process_reviews, ProcessInfo, ProcessReviewHistory as ReviewHistoryInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ==========================================
// Request / Response types
// ==========================================

#[derive(Deserialize)]
pub struct ProcessStateRequest {
    pub process_id: Option<Uuid>,
    pub template_id: Option<String>,
    pub flat_id: Option<i32>,
}

#[derive(Serialize)]
pub struct NextActionInfo {
    pub template_step_id: i32,
    pub name: String,
    pub assigned_to: String,
}

#[derive(Clone, Serialize)]
pub struct FieldHistoryEntry {
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub document_title: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct FieldValue {
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub document_title: Option<String>,
}

#[derive(Deserialize)]
pub struct DocumentUrlRequest {
    pub process_id: Uuid,
    pub document_id: Uuid,
}

#[derive(Serialize)]
pub struct DocumentUrlResponse {
    pub download_url: String,
}

#[derive(Serialize)]
pub struct FieldResponse {
    pub template_field_id: i32,
    pub name: String,
    pub r#type: String,
    pub filled_by: String,
    pub is_optional: bool,
    pub value: Option<FieldValue>,
    pub history: Vec<FieldHistoryEntry>,
}

#[derive(Serialize)]
pub struct StepResponse {
    pub template_step_id: i32,
    pub name: String,
    pub status: process::state_machine::StepStatus,
    pub is_review_required: bool,
    pub fields: Vec<FieldResponse>,
    pub substeps: Vec<StepResponse>,
}

#[derive(Serialize)]
pub struct ProcessStateResponse {
    pub process_id: Uuid,
    pub template_id: String,
    pub ownership_id: Option<i32>,
    pub is_completed: bool,
    pub next_actions: Vec<NextActionInfo>,
    pub reviews: Vec<ReviewHistoryInfo>,
    pub steps: Vec<StepResponse>,
}

#[derive(Deserialize)]
pub struct SaveStepRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub fields: Vec<SaveStepFieldInput>,
}

#[derive(Deserialize)]
pub struct SaveStepFieldInput {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct SaveStepResponse {
    pub status: String,
    pub message: String,
    pub step: SavedStepInfo,
}

#[derive(Serialize)]
pub struct SavedStepInfo {
    pub template_step_id: i32,
    pub is_completed: bool,
}

#[derive(Deserialize)]
pub struct AdminReviewRequest {
    pub process_id: Uuid,
    pub template_step_id: i32,
    pub is_approved: bool,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct AdminReviewResponse {
    pub status: String,
    pub message: String,
    pub is_completed: bool,
}

// ==========================================
// Shared helpers
// ==========================================

type ApiError = (StatusCode, Json<String>);

/// Load a process by ID, verify user authorization, and load its template.
/// Shared across save-step, review, and upload handlers.
async fn load_authorized_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    process_id: Uuid,
    is_admin: bool,
) -> Result<(ProcessInfo, process::ProcessTemplate), ApiError> {
    let info = get_process_info(conn, process_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json("Process not found".to_string())))?;

    if !is_admin && info.user_id != user.user_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json("Access Denied: You do not own this process".to_string()),
        ));
    }

    let template = process::ProcessTemplate::load_by_version(&info.template_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Template error: {}", e)),
        )
    })?;

    Ok((info, template))
}

/// For a resident user, resolve which ownership to use and find-or-create a process instance.
async fn resolve_or_init_process(
    conn: &mut sqlx::PgConnection,
    user: &authentication::User,
    template_id: &str,
    flat_id: Option<i32>,
) -> Result<(Uuid, String, Option<i32>), ApiError> {
    // Fetch active ownerships
    let active_ownerships = get_active_ownerships_for_user(conn, user.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    // Determine which ownership_id to use
    let selected_ownership = match flat_id {
        Some(fid) => {
            let oid = active_ownerships
                .iter()
                .find(|&&(_, flat)| flat == fid)
                .map(|&(oid, _)| oid)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json("No active ownership found for the specified flat_id".to_string()),
                    )
                })?;
            Some(oid)
        }
        None => {
            if active_ownerships.len() > 1 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json("Multiple active flats found. You must specify a flat_id".to_string()),
                ));
            } else if active_ownerships.is_empty() {
                None
            } else {
                Some(active_ownerships[0].0)
            }
        }
    };

    // Find existing process for this ownership + template
    let existing_pid =
        find_process_by_ownership_and_template(conn, selected_ownership, user.user_id, template_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    match existing_pid {
        Some(pid) => Ok((pid, template_id.to_string(), selected_ownership)),
        None => {
            // Auto-initialize a new process
            let template = process::ProcessTemplate::load_by_version(template_id).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(format!("Template error: {}", e)),
                )
            })?;

            let ctx = process::db::ProcessContext::new(conn, user.user_id, &template)
                .with_ownership(selected_ownership);
            let executor = ctx.init().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(format!("Failed to initialize process: {}", e)),
                )
            })?;
            Ok((
                executor.process_id,
                template_id.to_string(),
                selected_ownership,
            ))
        }
    }
}

/// Parse a process-specific multipart upload. Returns (process_id, title, file_bytes, content_type).
async fn parse_process_upload(
    multipart: &mut Multipart,
) -> Result<(Uuid, String, axum::body::Bytes, String), ApiError> {
    let mut file_data = None;
    let mut file_content_type = None;
    let mut file_name = None;
    let mut process_id = None;
    let mut title = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(format!("Multipart error: {}", e)),
        )
    })? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                file_content_type = field.content_type().map(|c| c.to_string());
                file_name = field.file_name().map(|f| f.to_string());
                let data = field.bytes().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(format!("Failed to read file: {}", e)),
                    )
                })?;
                file_data = Some(data);
            }
            "process_id" => {
                let text = field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(format!("Failed to read process_id: {}", e)),
                    )
                })?;
                process_id = Some(Uuid::parse_str(&text).map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(format!("Invalid process_id UUID: {}", e)),
                    )
                })?);
            }
            "title" => {
                let text = field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(format!("Failed to read title: {}", e)),
                    )
                })?;
                title = Some(text);
            }
            _ => {}
        }
    }

    let process_id = process_id.ok_or((
        StatusCode::BAD_REQUEST,
        Json("Missing process_id field in multipart request".to_string()),
    ))?;

    let file_bytes = file_data.ok_or((
        StatusCode::BAD_REQUEST,
        Json("Missing file field in multipart request".to_string()),
    ))?;

    let file_name = file_name.ok_or((
        StatusCode::BAD_REQUEST,
        Json("File name is required to determine file type.".to_string()),
    ))?;

    let title = title.unwrap_or_else(|| file_name.clone());

    if file_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json("Uploaded file cannot be empty".to_string()),
        ));
    }

    if file_bytes.len() > 10 * 1024 * 1024 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json("File size exceeds the 10MB limit".to_string()),
        ));
    }

    let ext = file_name
        .split('.')
        .next_back()
        .unwrap_or("")
        .to_lowercase();
    if !crate::handlers::documents::VALID_UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                "Invalid file type. Only PDF, Word, Excel, and Text files are allowed.".to_string(),
            ),
        ));
    }

    let content_type = file_content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    Ok((process_id, title, file_bytes, content_type))
}

/// Recursively map step templates to the response structure.
fn map_step_template(
    step_tpl: &process::ProcessStepTemplate,
    workflow: &process::state_machine::Workflow,
    field_history_map: &HashMap<i32, Vec<FieldHistoryEntry>>,
    latest_field_states: &HashMap<i32, process::state_machine::FieldState>,
    doc_title_map: &HashMap<Uuid, String>,
) -> StepResponse {
    let step_id = step_tpl.step_id;
    let step_state = workflow.steps.get(&step_id);
    let status = step_state
        .map(|s| s.status())
        .unwrap_or(process::state_machine::StepStatus::Locked);

    let mut fields = Vec::new();
    if let Some(ref field_tpls) = step_tpl.fields {
        for f in field_tpls {
            let field_id = f.field_id;
            let latest = latest_field_states.get(&field_id);
            let history = field_history_map
                .get(&field_id)
                .cloned()
                .unwrap_or_default();

            let value = latest.map(|l| {
                let title = l.document_id.and_then(|id| doc_title_map.get(&id)).cloned();
                FieldValue {
                    text_value: l.text_value.clone(),
                    document_id: l.document_id,
                    document_title: title,
                }
            });

            fields.push(FieldResponse {
                template_field_id: field_id,
                name: f.name.clone(),
                r#type: f.r#type.clone(),
                filled_by: f.filled_by.clone(),
                is_optional: f.is_optional,
                value,
                history,
            });
        }
    }

    let mut substeps = Vec::new();
    if let Some(ref sub_tpls) = step_tpl.substeps {
        for sub in sub_tpls {
            substeps.push(map_step_template(
                sub,
                workflow,
                field_history_map,
                latest_field_states,
                doc_title_map,
            ));
        }
    }

    StepResponse {
        template_step_id: step_id,
        name: step_tpl.name.clone(),
        status,
        is_review_required: step_tpl.is_review_required,
        fields,
        substeps,
    }
}

/// Build a flat map of step_id -> step_name from the template tree.
fn build_step_name_map(template: &process::ProcessTemplate) -> HashMap<i32, String> {
    let mut names = HashMap::new();
    names.insert(template.step_id, template.name.clone());

    fn traverse(
        substeps: &Option<Vec<process::ProcessStepTemplate>>,
        names: &mut HashMap<i32, String>,
    ) {
        if let Some(ref subs) = substeps {
            for sub in subs {
                names.insert(sub.step_id, sub.name.clone());
                traverse(&sub.substeps, names);
            }
        }
    }
    traverse(&template.substeps, &mut names);
    names
}

// ==========================================
// Route handlers
// ==========================================

/// POST /api/v1/process/state
pub async fn get_process_state(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<ProcessStateRequest>,
) -> Result<Json<ProcessStateResponse>, ApiError> {
    let mut tx = state
        .document_manager
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let is_admin = user.has_role(authentication::UserRole::Admin);

    // --- Resolve process_id, template_id, ownership_id ---
    let (process_id, template_id, ownership_id) = match payload.process_id {
        Some(pid) => {
            let (info, _) = load_authorized_process(&mut tx, &user, pid, is_admin).await?;
            (pid, info.template_id, info.ownership_id)
        }
        None => {
            if is_admin {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json("process_id is required for administrative users".to_string()),
                ));
            }
            let target_template_id = payload.template_id.clone().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json("Either process_id or template_id must be provided".to_string()),
                )
            })?;
            resolve_or_init_process(&mut tx, &user, &target_template_id, payload.flat_id).await?
        }
    };

    // --- Load template and workflow ---
    let template = process::ProcessTemplate::load_by_version(&template_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Template error: {}", e)),
        )
    })?;

    let ctx = process::db::ProcessContext::new(&mut tx, user.user_id, &template);
    let executor = ctx.load(process_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to load process: {}", e)),
        )
    })?;

    let workflow = executor.workflow;

    // --- Fetch documents (titles only), reviews, field history ---
    let docs = state
        .document_manager
        .get_documents_for_process(process_id)
        .await
        .unwrap_or_default();

    let doc_title_map: HashMap<Uuid, String> = docs
        .into_iter()
        .map(|doc| (doc.document_id, doc.title))
        .collect();

    let reviews = get_process_reviews(&mut tx, process_id)
        .await
        .unwrap_or_default();

    let pf_rows = get_field_history(&mut tx, process_id)
        .await
        .unwrap_or_default();

    let mut field_history_map: HashMap<i32, Vec<FieldHistoryEntry>> = HashMap::new();
    for row in pf_rows {
        let title = row
            .document_id
            .and_then(|id| doc_title_map.get(&id))
            .cloned();

        field_history_map
            .entry(row.template_field_id)
            .or_default()
            .push(FieldHistoryEntry {
                text_value: row.text_value,
                document_id: row.document_id,
                document_title: title,
                updated_at: row.updated_at,
            });
    }

    // Latest field states (first entry per field since ordered DESC)
    let mut latest_field_states = HashMap::new();
    for (fid, hist) in &field_history_map {
        if let Some(latest_entry) = hist.first() {
            latest_field_states.insert(
                *fid,
                process::state_machine::FieldState {
                    text_value: latest_entry.text_value.clone(),
                    document_id: latest_entry.document_id,
                },
            );
        }
    }

    // --- Compute next actions ---
    let step_info_map = process::state_machine::build_step_info_map(&template);
    let step_names = build_step_name_map(&template);
    let mut next_actions = Vec::new();

    for (&step_id, step_state) in &workflow.steps {
        if let Some(info) = step_info_map.get(&step_id) {
            if info.fields.is_empty() {
                continue;
            }
            match step_state.status() {
                process::state_machine::StepStatus::PendingReview => {
                    next_actions.push(NextActionInfo {
                        template_step_id: step_id,
                        name: step_names.get(&step_id).cloned().unwrap_or_default(),
                        assigned_to: "admin".to_string(),
                    });
                }
                process::state_machine::StepStatus::Open => {
                    let mut user_action = false;
                    let mut admin_action = false;
                    for f in &info.fields {
                        let is_filled = if let Some(f_state) = latest_field_states.get(&f.field_id)
                        {
                            process::fields::is_field_filled(
                                &f.r#type,
                                f_state.text_value.as_deref(),
                                f_state.document_id,
                            )
                        } else {
                            false
                        };
                        if !is_filled {
                            if f.filled_by == "user" {
                                user_action = true;
                            } else if f.filled_by == "admin" {
                                admin_action = true;
                            }
                        }
                    }
                    if user_action {
                        next_actions.push(NextActionInfo {
                            template_step_id: step_id,
                            name: step_names.get(&step_id).cloned().unwrap_or_default(),
                            assigned_to: "user".to_string(),
                        });
                    }
                    if admin_action {
                        next_actions.push(NextActionInfo {
                            template_step_id: step_id,
                            name: step_names.get(&step_id).cloned().unwrap_or_default(),
                            assigned_to: "admin".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // --- Build response tree ---
    let mut steps_res = Vec::new();
    if let Some(ref top_steps) = template.substeps {
        for top_step in top_steps {
            steps_res.push(map_step_template(
                top_step,
                &workflow,
                &field_history_map,
                &latest_field_states,
                &doc_title_map,
            ));
        }
    }

    let root_step_state = workflow.steps.get(&template.step_id);
    let is_completed = root_step_state
        .map(|s| s.status() == process::state_machine::StepStatus::Completed)
        .unwrap_or(false);

    // Commit transaction — needed for the auto-init path which may create a new process
    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    Ok(Json(ProcessStateResponse {
        process_id,
        template_id: template.template_id.clone(),
        ownership_id,
        is_completed,
        next_actions,
        reviews,
        steps: steps_res,
    }))
}

/// POST /api/v1/process/save-step
pub async fn save_process_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<SaveStepRequest>,
) -> Result<Json<SaveStepResponse>, ApiError> {
    let mut tx = state
        .document_manager
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let is_admin = user.has_role(authentication::UserRole::Admin);
    let (_, template) =
        load_authorized_process(&mut tx, &user, payload.process_id, is_admin).await?;

    // Validate document ownership
    for f in &payload.fields {
        if let Some(doc_id) = f.document_id {
            let is_valid: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM documents WHERE document_id = $1 AND process_id = $2 AND NOT is_deleted)"
            )
            .bind(doc_id)
            .bind(payload.process_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

            if !is_valid {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(format!(
                        "Document ID {} is invalid or does not belong to this process",
                        doc_id
                    )),
                ));
            }
        }
    }

    let ctx = process::db::ProcessContext::new(&mut tx, user.user_id, &template);
    let mut executor = ctx.load(payload.process_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to load process: {}", e)),
        )
    })?;

    let fields_submitted: Vec<process::state_machine::SaveFieldInput> = payload
        .fields
        .into_iter()
        .map(|f| process::state_machine::SaveFieldInput {
            template_field_id: f.template_field_id,
            text_value: f.text_value,
            document_id: f.document_id,
        })
        .collect();

    executor
        .save_step_fields(payload.template_step_id, &fields_submitted, is_admin)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let step_state = executor.workflow.steps.get(&payload.template_step_id);
    let is_completed = step_state
        .map(|s| s.status() == process::state_machine::StepStatus::Completed)
        .unwrap_or(false);

    // Commit transaction — all field inserts + step updates are atomic
    drop(executor);
    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    Ok(Json(SaveStepResponse {
        status: "success".to_string(),
        message: "Step saved successfully".to_string(),
        step: SavedStepInfo {
            template_step_id: payload.template_step_id,
            is_completed,
        },
    }))
}

/// POST /api/v1/admin/process/review
pub async fn admin_review_step(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<AdminReviewRequest>,
) -> Result<Json<AdminReviewResponse>, ApiError> {
    let mut tx = state
        .document_manager
        .pg_pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let (_, template) = load_authorized_process(&mut tx, &user, payload.process_id, true).await?;

    let ctx = process::db::ProcessContext::new(&mut tx, user.user_id, &template);
    let mut executor = ctx.load(payload.process_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to load process: {}", e)),
        )
    })?;

    executor
        .review_step(
            payload.template_step_id,
            Some(user.user_id),
            payload.is_approved,
            payload.notes.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e)))?;

    let step_state = executor.workflow.steps.get(&payload.template_step_id);
    let is_completed = step_state
        .map(|s| s.status() == process::state_machine::StepStatus::Completed)
        .unwrap_or(false);

    let message = if payload.is_approved {
        "Review recorded successfully. Step is marked as completed.".to_string()
    } else {
        "Review recorded successfully. Step is marked as incomplete.".to_string()
    };

    // Commit transaction — review insert + step updates are atomic
    drop(executor);
    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    Ok(Json(AdminReviewResponse {
        status: "success".to_string(),
        message,
        is_completed,
    }))
}

/// POST /api/v1/process/upload
pub async fn process_upload_document(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<crate::document::DocumentWithMetadata>), ApiError> {
    // Parse and validate the multipart upload
    let (process_id, title, file_bytes, content_type) =
        parse_process_upload(&mut multipart).await?;

    let is_admin = user.has_role(authentication::UserRole::Admin);
    let mut conn = state
        .document_manager
        .pg_pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    // Auth check
    load_authorized_process(&mut conn, &user, process_id, is_admin).await?;

    // Create document record
    let result = state
        .document_manager
        .create_document_for_process_atomic(process_id, &title, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    // Upload to S3
    let key = crate::document::derive_s3_key(
        Some(process_id),
        DocumentAccessLevel::Private,
        result.document.document_id,
    );

    if let Err(err) = state
        .document_manager
        .s3_accessor
        .upload_file(
            &key,
            aws_sdk_s3::primitives::ByteStream::from(file_bytes),
            &content_type,
        )
        .await
    {
        let _ = state
            .document_manager
            .delete_document_permanently(result.document.document_id)
            .await;

        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to upload document file to S3: {}", err)),
        ));
    }

    Ok((StatusCode::CREATED, Json(result)))
}

/// POST /api/v1/process/download-url
/// On-demand presigned download URL for a process document.
pub async fn process_download_url(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    Json(payload): Json<DocumentUrlRequest>,
) -> Result<Json<DocumentUrlResponse>, ApiError> {
    let mut conn = state
        .document_manager
        .pg_pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?;

    let is_admin = user.has_role(authentication::UserRole::Admin);
    load_authorized_process(&mut conn, &user, payload.process_id, is_admin).await?;

    // Verify document belongs to this process
    let doc: Document = sqlx::query_as(
        "SELECT document_id, title, description, category, document_type, access_level, \
         restricted_committee_roles, uploaded_date, is_deleted, is_user_uploaded, process_id \
         FROM documents WHERE document_id = $1 AND process_id = $2 AND NOT is_deleted",
    )
    .bind(payload.document_id)
    .bind(payload.process_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json("Document not found or does not belong to this process".to_string()),
        )
    })?;

    let url = state
        .document_manager
        .generate_presigned_url(&doc)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    Ok(Json(DocumentUrlResponse { download_url: url }))
}
