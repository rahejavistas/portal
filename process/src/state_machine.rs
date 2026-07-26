use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// ==========================================
// Core enums
// ==========================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    Locked,
    Open,
    PendingReview,
    Completed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessTurn {
    User,
    Admin,
    None,
}

// ==========================================
// DB state (loaded from database, fed into pure functions)
// ==========================================

#[derive(Debug, Clone)]
pub struct StepStateDb {
    pub is_completed: bool,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct FieldState {
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ReviewState {
    pub is_approved: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessStateInput {
    pub steps: HashMap<i32, StepStateDb>,
    pub fields: HashMap<i32, FieldState>,
    pub reviews: HashMap<i32, Vec<ReviewState>>,
    /// document_ids with at least one PENDING signature requirement (any required
    /// designation not yet signed) — a document field whose current document_id is in
    /// this set is treated as not-filled by `check_step_completed`, on top of the
    /// existing `is_field_filled` check.
    pub blocking_document_ids: HashSet<Uuid>,
}

// ==========================================
// Operation inputs / outputs
// ==========================================

#[derive(Debug, Clone)]
pub struct SaveFieldInput {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
}

/// Result of validating and computing a step save operation.
#[derive(Debug, Clone)]
pub struct SaveResult {
    /// (template_step_id, is_completed)
    pub steps_to_update: Vec<(i32, bool)>,
    /// (template_field_id, text_value, document_id)
    pub fields_to_save: Vec<(i32, Option<String>, Option<Uuid>)>,
    pub new_status: StepStatus,
}

// ==========================================
// Template info (flattened from tree)
// ==========================================

#[derive(Debug, Clone)]
pub struct StepInfo {
    pub step_id: i32,
    pub name: String,
    pub parent_id: Option<i32>,
    pub is_review_required: bool,
    pub is_optional: bool,
    pub fields: Vec<crate::ProcessFieldTemplate>,
    pub substep_ids: Vec<i32>,
}

// ==========================================
// Template helpers
// ==========================================

/// Build a flat lookup map of step info from the tree template.
pub fn build_step_info_map(template: &crate::ProcessTemplate) -> HashMap<i32, StepInfo> {
    let mut map = HashMap::new();

    let root_substep_ids = template
        .substeps
        .as_ref()
        .map(|subs| subs.iter().map(|s| s.step_id).collect())
        .unwrap_or_default();

    map.insert(
        template.step_id,
        StepInfo {
            step_id: template.step_id,
            name: template.name.clone(),
            parent_id: None,
            is_review_required: template.is_review_required,
            is_optional: template.is_optional,
            fields: template.fields.clone().unwrap_or_default(),
            substep_ids: root_substep_ids,
        },
    );

    if let Some(ref subs) = template.substeps {
        for sub in subs {
            insert_step_info_recursive(&mut map, sub, Some(template.step_id));
        }
    }

    map
}

fn insert_step_info_recursive(
    map: &mut HashMap<i32, StepInfo>,
    step: &crate::ProcessStepTemplate,
    parent_id: Option<i32>,
) {
    let substep_ids = step
        .substeps
        .as_ref()
        .map(|subs| subs.iter().map(|s| s.step_id).collect())
        .unwrap_or_default();

    map.insert(
        step.step_id,
        StepInfo {
            step_id: step.step_id,
            name: step.name.clone(),
            parent_id,
            is_review_required: step.is_review_required,
            is_optional: step.is_optional,
            fields: step.fields.clone().unwrap_or_default(),
            substep_ids,
        },
    );

    if let Some(ref subs) = step.substeps {
        for sub in subs {
            insert_step_info_recursive(map, sub, Some(step.step_id));
        }
    }
}

// ==========================================
// Pure computation functions
// ==========================================

/// Check if all mandatory fields in a step are filled.
pub fn check_step_completed(
    step_id: i32,
    info_map: &HashMap<i32, StepInfo>,
    fields: &HashMap<i32, FieldState>,
    blocking_document_ids: &HashSet<Uuid>,
) -> bool {
    let info = match info_map.get(&step_id) {
        Some(i) => i,
        None => return false,
    };
    for field in &info.fields {
        if !field.is_optional {
            let is_filled = if let Some(fs) = fields.get(&field.field_id) {
                let base_filled = crate::fields::is_field_filled(
                    &field.r#type,
                    fs.text_value.as_deref(),
                    fs.document_id,
                );
                let blocked = fs
                    .document_id
                    .map(|id| blocking_document_ids.contains(&id))
                    .unwrap_or(false);
                base_filled && !blocked
            } else {
                false
            };
            if !is_filled {
                return false;
            }
        }
    }
    true
}

/// Compute the status of every step based on DB state and reviews.
///
/// Rules:
/// - `Locked` if the preceding top-level step is not `Completed`.
/// - `Completed` if completed in DB and approved (or no review required).
/// - `PendingReview` if completed in DB but not yet approved.
/// - `Open` if unlocked but not yet completed.
pub fn compute_statuses(
    template: &crate::ProcessTemplate,
    state_input: &ProcessStateInput,
) -> HashMap<i32, StepStatus> {
    let step_info_map = build_step_info_map(template);
    let mut statuses = HashMap::new();

    let is_approved = |step_id: i32| -> bool {
        let completed = state_input
            .steps
            .get(&step_id)
            .map(|s| s.is_completed)
            .unwrap_or(false);
        if !completed {
            return false;
        }
        let info = match step_info_map.get(&step_id) {
            Some(i) => i,
            None => return false,
        };
        if info.is_review_required {
            state_input
                .reviews
                .get(&step_id)
                .and_then(|revs| revs.iter().max_by_key(|r| r.created_at))
                .map(|r| r.is_approved)
                .unwrap_or(false)
        } else {
            true
        }
    };

    // Safe unwrap: template.step_id is the root step.
    let root_info = step_info_map.get(&template.step_id).unwrap();
    let top_step_ids = &root_info.substep_ids;

    let mut prev_approved = true;

    for &step_id in top_step_ids {
        // Safe unwrap: step_id comes from root_info.substep_ids.
        let info = step_info_map.get(&step_id).unwrap();

        let status = if !prev_approved {
            StepStatus::Locked
        } else {
            let completed = state_input
                .steps
                .get(&step_id)
                .map(|s| s.is_completed)
                .unwrap_or(false);
            if completed {
                if info.is_review_required {
                    if is_approved(step_id) {
                        StepStatus::Completed
                    } else {
                        StepStatus::PendingReview
                    }
                } else {
                    StepStatus::Completed
                }
            } else {
                StepStatus::Open
            }
        };

        statuses.insert(step_id, status);

        for &sub_id in &info.substep_ids {
            let sub_status = match status {
                StepStatus::Locked => StepStatus::Locked,
                StepStatus::Completed | StepStatus::PendingReview => StepStatus::Completed,
                StepStatus::Open => {
                    let sub_completed = state_input
                        .steps
                        .get(&sub_id)
                        .map(|s| s.is_completed)
                        .unwrap_or(false);
                    if sub_completed {
                        StepStatus::Completed
                    } else {
                        StepStatus::Open
                    }
                }
            };
            statuses.insert(sub_id, sub_status);
        }

        prev_approved = status == StepStatus::Completed;
    }

    let root_completed = top_step_ids
        .iter()
        .all(|&id| statuses.get(&id) == Some(&StepStatus::Completed));
    statuses.insert(
        template.step_id,
        if root_completed {
            StepStatus::Completed
        } else {
            StepStatus::Open
        },
    );

    statuses
}

/// Determine whose turn it is to act on the process.
///
/// Simple rules:
/// 1. If root is Completed → None
/// 2. If any step is PendingReview → Admin
/// 3. If any Open step has admin fields → Admin
/// 4. Otherwise → User
pub fn determine_turn(template: &crate::ProcessTemplate, state: &ProcessStateInput) -> ProcessTurn {
    let statuses = compute_statuses(template, state);
    let info_map = build_step_info_map(template);

    if statuses.get(&template.step_id) == Some(&StepStatus::Completed) {
        return ProcessTurn::None;
    }

    for &status in statuses.values() {
        if status == StepStatus::PendingReview {
            return ProcessTurn::Admin;
        }
    }

    for (&step_id, &status) in &statuses {
        if status == StepStatus::Open {
            if let Some(info) = info_map.get(&step_id) {
                if info.fields.iter().any(|f| f.filled_by == "admin") {
                    return ProcessTurn::Admin;
                }
            }
        }
    }

    ProcessTurn::User
}

/// Validate fields and compute what DB updates are needed to save a step.
///
/// Does NOT touch the database — returns a `SaveResult` for the caller to persist.
pub fn validate_save(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
    fields_submitted: &[SaveFieldInput],
    is_admin: bool,
) -> Result<SaveResult, String> {
    let info_map = build_step_info_map(template);
    let statuses = compute_statuses(template, state);

    let status = statuses
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not found", step_id))?;

    let info = info_map
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not in template", step_id))?;

    // Check step is editable
    let is_parent_open = info
        .parent_id
        .and_then(|pid| statuses.get(&pid))
        .map(|s| *s == StepStatus::Open)
        .unwrap_or(false);

    match status {
        StepStatus::Open => {}
        StepStatus::Completed if is_admin || is_parent_open => {}
        StepStatus::PendingReview if is_admin => {}
        _ => return Err("Step is not open for editing".to_string()),
    }

    // Validate each field and build the save list
    let mut fields_to_save = Vec::new();
    let mut updated_fields = state.fields.clone();

    for submitted in fields_submitted {
        let field_meta = info
            .fields
            .iter()
            .find(|f| f.field_id == submitted.template_field_id)
            .ok_or_else(|| {
                format!(
                    "Field ID {} does not belong to step {}",
                    submitted.template_field_id, step_id
                )
            })?;

        if field_meta.filled_by == "admin" && !is_admin {
            return Err(format!(
                "Field '{}' can only be filled by an admin",
                field_meta.name
            ));
        }

        crate::fields::validate_field(
            &field_meta.r#type,
            submitted.text_value.as_deref(),
            submitted.document_id,
            field_meta.is_optional,
        )?;

        updated_fields.insert(
            submitted.template_field_id,
            FieldState {
                text_value: submitted.text_value.clone(),
                document_id: submitted.document_id,
            },
        );

        fields_to_save.push((
            submitted.template_field_id,
            submitted.text_value.clone(),
            submitted.document_id,
        ));
    }

    let updated_state = ProcessStateInput {
        fields: updated_fields,
        ..state.clone()
    };
    let steps_to_update = compute_completion_updates(template, &updated_state, step_id);
    let target_completed = steps_to_update
        .iter()
        .find(|(sid, _)| *sid == step_id)
        .map(|(_, completed)| *completed)
        .unwrap_or(false);

    let new_status = if target_completed {
        if info.is_review_required {
            StepStatus::PendingReview
        } else {
            StepStatus::Completed
        }
    } else {
        StepStatus::Open
    };

    Ok(SaveResult {
        steps_to_update,
        fields_to_save,
        new_status,
    })
}

/// Given a step's freshly-computed completion and the rest of process state, returns
/// the (template_step_id, is_completed) pairs to persist for that step and (if
/// applicable) its parent. Shared by `validate_save` and by callers reacting to
/// out-of-band completion changes.
pub fn compute_completion_updates(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
) -> Vec<(i32, bool)> {
    let info_map = build_step_info_map(template);
    let info = match info_map.get(&step_id) {
        Some(i) => i,
        None => return vec![],
    };

    let target_completed = check_step_completed(
        step_id,
        &info_map,
        &state.fields,
        &state.blocking_document_ids,
    );
    let mut steps_to_update = vec![(step_id, target_completed)];

    if let Some(parent_id) = info.parent_id {
        if parent_id != template.step_id {
            if let Some(parent_info) = info_map.get(&parent_id) {
                let all_siblings_completed = parent_info.substep_ids.iter().all(|&sub_id| {
                    if sub_id == step_id {
                        target_completed
                    } else {
                        state
                            .steps
                            .get(&sub_id)
                            .map(|s| s.is_completed)
                            .unwrap_or(false)
                    }
                });
                steps_to_update.push((parent_id, all_siblings_completed));
            }
        }
    }

    steps_to_update
}

/// Compute step updates for an admin review (approve or reject).
///
/// Returns `(steps_to_update, is_step_completed)` where each update is
/// `(template_step_id, is_completed)`.
pub fn compute_review_updates(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
    is_approved: bool,
) -> Result<(Vec<(i32, bool)>, bool), String> {
    let statuses = compute_statuses(template, state);
    let info_map = build_step_info_map(template);

    let status = statuses
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not found", step_id))?;

    // Allow admin to review steps that are PENDING_REVIEW or OPEN
    // (e.g., admin can approve an OPEN step directly without requiring user re-submission)
    match status {
        StepStatus::PendingReview | StepStatus::Open => {}
        _ => return Err("Step must be open or pending review for admin action".to_string()),
    }

    let mut steps_to_update = Vec::new();

    if is_approved {
        // Mark step (and parent chain) as completed
        steps_to_update.push((step_id, true));

        // If the step was OPEN (not yet completed), propagate completion to parent
        if *status == StepStatus::Open {
            if let Some(info) = info_map.get(&step_id) {
                if let Some(parent_id) = info.parent_id {
                    if parent_id != template.step_id {
                        if let Some(parent_info) = info_map.get(&parent_id) {
                            let all_siblings_completed =
                                parent_info.substep_ids.iter().all(|&sub_id| {
                                    if sub_id == step_id {
                                        true
                                    } else {
                                        state
                                            .steps
                                            .get(&sub_id)
                                            .map(|s| s.is_completed)
                                            .unwrap_or(false)
                                    }
                                });
                            steps_to_update.push((parent_id, all_siblings_completed));
                        }
                    }
                }
            }
        }
        Ok((steps_to_update, true))
    } else {
        steps_to_update.push((step_id, false));
        // Walk up parent chain to reset completion
        let mut curr = info_map.get(&step_id).unwrap();
        while let Some(parent_id) = curr.parent_id {
            if parent_id == template.step_id {
                break;
            }
            steps_to_update.push((parent_id, false));
            match info_map.get(&parent_id) {
                Some(p) => curr = p,
                None => break,
            }
        }
        Ok((steps_to_update, false))
    }
}

/// Compute which steps need to be reset when reopening a completed/pending step.
///
/// Returns the list of step IDs to reset (set is_completed = false).
/// Also returns which of those are currently closed (need rejection reviews).
pub fn compute_reopen_updates(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
) -> Result<Vec<i32>, String> {
    let statuses = compute_statuses(template, state);
    let info_map = build_step_info_map(template);

    let status = statuses
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not found", step_id))?;

    match status {
        StepStatus::Completed | StepStatus::PendingReview => {}
        _ => {
            return Err(
                "Step is not closed; only completed or pending review steps can be reopened"
                    .to_string(),
            )
        }
    }

    let mut steps_to_reset = vec![step_id];
    let mut curr = info_map.get(&step_id).unwrap();
    while let Some(parent_id) = curr.parent_id {
        if parent_id == template.step_id {
            break;
        }
        steps_to_reset.push(parent_id);
        match info_map.get(&parent_id) {
            Some(p) => curr = p,
            None => break,
        }
    }

    Ok(steps_to_reset)
}

// ==========================================
// Tests
// ==========================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_statuses_initial() {
        let template = crate::test_fixtures::tenant_onboarding_fixture();
        let state = ProcessStateInput::default();

        let statuses = compute_statuses(&template, &state);

        assert_eq!(statuses.get(&2), Some(&StepStatus::Open));
        assert_eq!(statuses.get(&3), Some(&StepStatus::Open));
        assert_eq!(statuses.get(&7), Some(&StepStatus::Locked));
        assert_eq!(statuses.get(&1), Some(&StepStatus::Open));
    }

    #[test]
    fn test_validate_save_and_completion() {
        let template = crate::test_fixtures::tenant_onboarding_fixture();
        let state = ProcessStateInput::default();

        let doc_id = Uuid::new_v4();
        let fields = vec![
            SaveFieldInput {
                template_field_id: 101,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 102,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 103,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 105,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 106,
                text_value: None,
                document_id: Some(doc_id),
            },
        ];

        let result = validate_save(&template, &state, 3, &fields, false).unwrap();

        assert_eq!(result.new_status, StepStatus::Completed);
        assert!(result.steps_to_update.contains(&(3, true)));
        // Parent step 2 not all siblings done yet
        assert!(result.steps_to_update.contains(&(2, false)));
    }

    #[test]
    fn test_determine_turn() {
        let template = crate::test_fixtures::tenant_onboarding_fixture();

        // 1. Initial: user fields unfilled → User
        let mut state = ProcessStateInput::default();
        assert_eq!(determine_turn(&template, &state), ProcessTurn::User);

        // 2. Some substeps done, others have unfilled user fields → User
        let doc_id = Uuid::new_v4();
        for fid in [101, 102, 103, 105, 106] {
            state.fields.insert(
                fid,
                FieldState {
                    text_value: None,
                    document_id: Some(doc_id),
                },
            );
        }
        state.steps.insert(
            3,
            StepStateDb {
                is_completed: true,
                started_at: None,
            },
        );
        assert_eq!(determine_turn(&template, &state), ProcessTurn::User);

        // 3. All substeps done, Step 1 is PendingReview → Admin
        for fid in [107, 108, 109, 110] {
            state.fields.insert(
                fid,
                FieldState {
                    text_value: None,
                    document_id: Some(doc_id),
                },
            );
        }
        for sid in [4, 5, 6, 2] {
            state.steps.insert(
                sid,
                StepStateDb {
                    is_completed: true,
                    started_at: None,
                },
            );
        }
        assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

        // 4. Step 1 approved, Step 2 Open with admin field → Admin
        state.reviews.insert(
            2,
            vec![ReviewState {
                is_approved: true,
                created_at: chrono::Utc::now(),
            }],
        );
        state.steps.insert(
            7,
            StepStateDb {
                is_completed: false,
                started_at: None,
            },
        );
        assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

        // 5. Step 2 completed+approved, Step 3 Open with admin field → Admin
        state.fields.insert(
            111,
            FieldState {
                text_value: Some("Done".into()),
                document_id: None,
            },
        );
        state.steps.insert(
            7,
            StepStateDb {
                is_completed: true,
                started_at: None,
            },
        );
        state.reviews.insert(
            7,
            vec![ReviewState {
                is_approved: true,
                created_at: chrono::Utc::now(),
            }],
        );
        assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

        // 6. Step 3 done, Step 4 Open with user fields → User
        state.fields.insert(
            112,
            FieldState {
                text_value: None,
                document_id: Some(doc_id),
            },
        );
        state.steps.insert(
            8,
            StepStateDb {
                is_completed: true,
                started_at: None,
            },
        );
        state.reviews.insert(
            8,
            vec![ReviewState {
                is_approved: true,
                created_at: chrono::Utc::now(),
            }],
        );
        assert_eq!(determine_turn(&template, &state), ProcessTurn::User);
    }

    #[test]
    fn test_compute_review_updates_allows_open_steps() {
        let template = crate::test_fixtures::tenant_onboarding_fixture();
        let doc_id = Uuid::new_v4();

        // Build state: all Step 1 substeps completed, Step 1 (id=2) pending review
        let mut state = ProcessStateInput::default();
        for fid in [101, 102, 103, 105, 106, 107, 108, 109, 110] {
            state.fields.insert(
                fid,
                FieldState {
                    text_value: None,
                    document_id: Some(doc_id),
                },
            );
        }
        for sid in [3, 4, 5, 6, 2] {
            state.steps.insert(
                sid,
                StepStateDb {
                    is_completed: true,
                    started_at: None,
                },
            );
        }

        // 1. Classic path: approve PENDING_REVIEW step succeeds
        let result = compute_review_updates(&template, &state, 2, true);
        assert!(result.is_ok());
        let (updates, completed) = result.unwrap();
        assert!(completed);
        assert!(updates.contains(&(2, true)));

        // 2. Reject PENDING_REVIEW → step reopened
        let result = compute_review_updates(&template, &state, 2, false);
        assert!(result.is_ok());
        let (updates, completed) = result.unwrap();
        assert!(!completed);
        assert!(updates.contains(&(2, false)));

        // 3. After rejection, step 2 is now OPEN (simulate by clearing completion)
        state.steps.insert(
            2,
            StepStateDb {
                is_completed: false,
                started_at: None,
            },
        );
        // Verify step 2 is indeed OPEN now
        let statuses = compute_statuses(&template, &state);
        assert_eq!(statuses.get(&2), Some(&StepStatus::Open));

        // 4. Admin can approve an OPEN step directly (no user re-submission needed)
        let result = compute_review_updates(&template, &state, 2, true);
        assert!(
            result.is_ok(),
            "Admin should be able to approve an OPEN step"
        );
        let (updates, completed) = result.unwrap();
        assert!(completed);
        assert!(updates.contains(&(2, true)));

        // 5. Admin can also reject an OPEN step
        let result = compute_review_updates(&template, &state, 2, false);
        assert!(
            result.is_ok(),
            "Admin should be able to reject an OPEN step"
        );
        let (_updates, completed) = result.unwrap();
        assert!(!completed);

        // 6. LOCKED steps are still blocked
        let result = compute_review_updates(&template, &state, 7, true);
        assert!(result.is_err(), "LOCKED steps should not be reviewable");
        assert!(result.unwrap_err().contains("open or pending review"));

        // 7. COMPLETED steps are still blocked
        let result = compute_review_updates(&template, &state, 3, true);
        assert!(result.is_err(), "COMPLETED steps should not be reviewable");
    }
}
