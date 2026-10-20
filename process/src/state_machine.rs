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

impl ProcessTurn {
    /// Value stored in `processes.current_turn`.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            ProcessTurn::User => "user",
            ProcessTurn::Admin => "admin",
            ProcessTurn::None => "none",
        }
    }
}

/// Who's calling, resolved fresh per request — never cached.
#[derive(Debug, Clone, Default)]
pub struct ActorContext {
    pub is_admin: bool,
    pub designation_codes: HashSet<String>,
    /// Every wing's representative designation code, system-wide.
    pub wing_representative_codes: HashSet<String>,
    /// Whether the caller is the active tenant of this process's own flat.
    pub is_active_tenant_for_this_flat: bool,
    /// No active tenant mapped at all — lets an admin fill `Tenant` steps instead.
    pub tenant_missing_for_this_flat: bool,
    /// Whether the caller owns this process (`processes.user_id`) and is still the
    /// flat's active owner.
    pub is_process_owner: bool,
}

impl ActorContext {
    /// Which of `designation_codes` is also a wing-representative code, if any.
    fn matching_wing_representative_code(&self) -> Option<&str> {
        self.designation_codes
            .iter()
            .find(|c| self.wing_representative_codes.contains(c.as_str()))
            .map(|s| s.as_str())
    }

    /// `Designation`-owned steps are exclusive to that designation. `User`-owned
    /// steps are exclusive to the process's own owner (or an admin).
    pub fn may_act_on(&self, owner: &crate::StepOwner) -> bool {
        match owner {
            crate::StepOwner::User => self.is_process_owner || self.is_admin,
            crate::StepOwner::Admin => self.is_admin,
            crate::StepOwner::Designation { code } => self.designation_codes.contains(code),
            crate::StepOwner::WingRepresentative => {
                self.matching_wing_representative_code().is_some()
            }
            crate::StepOwner::Tenant => {
                self.is_active_tenant_for_this_flat
                    || (self.is_admin && self.tenant_missing_for_this_flat)
            }
        }
    }

    /// Like `may_act_on`, but a `User` step holding a `Declaration` field is the
    /// owner's alone — an admin can't declare on their behalf.
    pub fn may_act_on_step(
        &self,
        owner: &crate::StepOwner,
        fields: &[crate::ProcessFieldTemplate],
    ) -> bool {
        let has_declaration = fields
            .iter()
            .any(|f| f.r#type == crate::FieldType::Declaration);
        match owner {
            crate::StepOwner::User if has_declaration => self.is_process_owner,
            _ => self.may_act_on(owner),
        }
    }

    /// For reviewer-only actions (review, reopen, admin notes) — unlike `may_act_on`,
    /// a `User`-owned step is never self-actionable; only an admin or the owning
    /// designation may act. `Tenant` is admin-only too.
    pub fn may_review(&self, owner: &crate::StepOwner) -> bool {
        match owner {
            crate::StepOwner::User | crate::StepOwner::Admin | crate::StepOwner::Tenant => {
                self.is_admin
            }
            crate::StepOwner::Designation { code } => {
                self.is_admin || self.designation_codes.contains(code)
            }
            crate::StepOwner::WingRepresentative => {
                self.is_admin || self.matching_wing_representative_code().is_some()
            }
        }
    }
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
    /// document_ids with a PENDING signature requirement — treated as not-filled.
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
    /// The designation code, snapshotted at write time, if this step is `Designation`-owned.
    pub acted_as_designation: Option<String>,
}

// ==========================================
// Template info (flattened from tree)
// ==========================================

#[derive(Debug, Clone)]
pub struct StepInfo {
    pub step_id: i32,
    pub name: String,
    pub parent_id: Option<i32>,
    pub is_optional: bool,
    pub filled_by: crate::StepOwner,
    pub fields: Vec<crate::ProcessFieldTemplate>,
    pub substep_ids: Vec<i32>,
}

impl StepInfo {
    /// Only a top-level, `User`-owned Stage ever reaches `PendingReview`.
    pub fn requires_review(&self, root_step_id: i32) -> bool {
        matches!(self.filled_by, crate::StepOwner::User) && self.parent_id == Some(root_step_id)
    }
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
            is_optional: template.is_optional,
            filled_by: template.filled_by.clone(),
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
            is_optional: step.is_optional,
            filled_by: step.filled_by.clone(),
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

/// A template with a `Tenant` stage must also require the owner's declaration: a
/// required `User` step holding a `Declaration` field.
pub fn tenant_stage_lacks_owner_declaration(template: &crate::ProcessTemplate) -> bool {
    let map = build_step_info_map(template);
    let is_required = |info: &StepInfo| {
        let mut current = Some(info);
        while let Some(step) = current {
            if step.is_optional {
                return false;
            }
            current = step.parent_id.and_then(|pid| map.get(&pid));
        }
        true
    };
    let has_tenant_stage = map
        .values()
        .any(|i| matches!(i.filled_by, crate::StepOwner::Tenant));
    let has_owner_declaration = map.values().any(|i| {
        i.parent_id.is_some()
            && matches!(i.filled_by, crate::StepOwner::User)
            && i.fields
                .iter()
                .any(|f| f.r#type == crate::FieldType::Declaration)
            && is_required(i)
    });
    has_tenant_stage && !has_owner_declaration
}

// ==========================================
// Pure computation functions
// ==========================================

/// Check if all mandatory fields in a step are filled.
pub fn check_step_completed(
    step_id: i32,
    info_map: &HashMap<i32, StepInfo>,
    state: &ProcessStateInput,
) -> bool {
    let info = match info_map.get(&step_id) {
        Some(i) => i,
        None => return false,
    };
    for field in &info.fields {
        if !field.is_optional {
            let is_filled = if let Some(fs) = state.fields.get(&field.field_id) {
                let base_filled = crate::fields::is_field_filled(
                    field.r#type,
                    fs.text_value.as_deref(),
                    fs.document_id,
                    field.requirements.as_ref(),
                );
                let blocked = fs
                    .document_id
                    .map(|id| state.blocking_document_ids.contains(&id))
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
    info.substep_ids.iter().all(|sub_id| {
        state
            .steps
            .get(sub_id)
            .map(|s| s.is_completed)
            .unwrap_or(false)
    })
}

/// Locked if the preceding top-level step isn't Completed; Completed if done+approved
/// (or no review required); PendingReview if done but not yet approved; else Open.
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
        if info.requires_review(template.step_id) {
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
                if info.requires_review(template.step_id) {
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

/// Step IDs that are `Open` in `after` but weren't in `before` — used to fire a
/// "your turn" notification only on a genuine transition, not on every read.
pub fn newly_opened_steps(
    before: &HashMap<i32, StepStatus>,
    after: &HashMap<i32, StepStatus>,
) -> Vec<i32> {
    after
        .iter()
        .filter(|&(step_id, &status)| {
            status == StepStatus::Open && before.get(step_id) != Some(&StepStatus::Open)
        })
        .map(|(&step_id, _)| step_id)
        .collect()
}

/// None if root is Completed; Admin if any step is PendingReview or an Open step has
/// admin fields; else User.
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
                if info.filled_by.turn() == ProcessTurn::Admin {
                    return ProcessTurn::Admin;
                }
            }
        }
    }

    ProcessTurn::User
}

/// Whether `actor` can personally act on some step in this process right now.
pub fn is_pending_on_actor(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    actor: &ActorContext,
) -> bool {
    let statuses = compute_statuses(template, state);
    let info_map = build_step_info_map(template);

    statuses.iter().any(|(step_id, status)| {
        let Some(info) = info_map.get(step_id) else {
            return false;
        };
        match status {
            StepStatus::Open => {
                !matches!(info.filled_by, crate::StepOwner::User)
                    && actor.may_act_on(&info.filled_by)
            }
            StepStatus::PendingReview => actor.may_review(&info.filled_by),
            StepStatus::Locked | StepStatus::Completed => false,
        }
    })
}

/// Validates fields and computes DB updates for a step save. Does not touch the DB.
pub fn validate_save(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
    fields_submitted: &[SaveFieldInput],
    actor: &ActorContext,
) -> Result<SaveResult, String> {
    let info_map = build_step_info_map(template);
    let statuses = compute_statuses(template, state);

    let status = statuses
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not found", step_id))?;

    let info = info_map
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not in template", step_id))?;

    let may_act = actor.may_act_on_step(&info.filled_by, &info.fields);

    // Check step is editable
    let is_parent_open = info
        .parent_id
        .and_then(|pid| statuses.get(&pid))
        .map(|s| *s == StepStatus::Open)
        .unwrap_or(false);

    // Editing an already-Completed/PendingReview step is an admin override, distinct from
    // `may_act` (ownership of the step) — a resident must never re-edit their own
    // already-approved User-owned step just because they "own" it.
    match status {
        StepStatus::Open => {}
        StepStatus::Completed if actor.is_admin || is_parent_open => {}
        StepStatus::PendingReview if actor.is_admin => {}
        _ => return Err("Step is not open for editing".to_string()),
    }

    if !may_act {
        return Err(match &info.filled_by {
            crate::StepOwner::User => {
                "This stage can only be filled by the process's own owner".to_string()
            }
            crate::StepOwner::Admin => "This stage can only be filled by an admin".to_string(),
            crate::StepOwner::Designation { code } => format!(
                "This stage can only be filled by someone holding the '{}' designation",
                code
            ),
            crate::StepOwner::WingRepresentative => {
                "This stage can only be filled by a wing representative".to_string()
            }
            crate::StepOwner::Tenant => {
                "This stage can only be filled by the flat's current tenant, or an admin if no tenant is mapped yet".to_string()
            }
        });
    }

    let acted_as_designation = match &info.filled_by {
        crate::StepOwner::Designation { code } => Some(code.clone()),
        crate::StepOwner::WingRepresentative => actor
            .matching_wing_representative_code()
            .map(|c| c.to_string()),
        _ => None,
    };

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

        crate::fields::validate_field(
            field_meta.r#type,
            submitted.text_value.as_deref(),
            submitted.document_id,
            field_meta.is_optional,
            field_meta.options.as_deref(),
            field_meta.requirements.as_ref(),
        )?;

        updated_fields.insert(
            submitted.template_field_id,
            FieldState {
                text_value: submitted.text_value.clone(),
                document_id: submitted.document_id,
            },
        );

        // NoDues is never resident-entered — load_state re-injects the real, live
        // amount on every read regardless, so persisting a submitted value here
        // would just be dead data sitting in process_fields.
        if field_meta.r#type != crate::FieldType::NoDues {
            fields_to_save.push((
                submitted.template_field_id,
                submitted.text_value.clone(),
                submitted.document_id,
            ));
        }
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
        if info.requires_review(template.step_id) {
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
        acted_as_designation,
    })
}

/// Marks `step_id` completed and, if all its siblings are also complete, marks the
/// immediate parent too. Never walks past one level.
fn cascade_completion_one_level(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    info_map: &HashMap<i32, StepInfo>,
    step_id: i32,
    target_completed: bool,
) -> Vec<(i32, bool)> {
    let mut steps_to_update = vec![(step_id, target_completed)];

    if let Some(info) = info_map.get(&step_id) {
        if let Some(parent_id) = info.parent_id {
            if parent_id != template.step_id && info_map.contains_key(&parent_id) {
                // Overlay this save's not-yet-persisted completion onto a copy of
                // `state`, so a parent with fields of its own (not just substeps)
                // is only marked complete when those fields are filled too.
                let mut overlaid_steps = state.steps.clone();
                overlaid_steps.insert(
                    step_id,
                    StepStateDb {
                        is_completed: target_completed,
                        started_at: None,
                    },
                );
                let overlaid_state = ProcessStateInput {
                    steps: overlaid_steps,
                    ..state.clone()
                };
                let parent_completed = check_step_completed(parent_id, info_map, &overlaid_state);
                steps_to_update.push((parent_id, parent_completed));
            }
        }
    }

    steps_to_update
}

/// Resets `step_id` and every ancestor up to the template root to incomplete.
fn cascade_incomplete_to_root(
    template: &crate::ProcessTemplate,
    info_map: &HashMap<i32, StepInfo>,
    step_id: i32,
) -> Vec<i32> {
    let mut steps = vec![step_id];
    let Some(mut curr) = info_map.get(&step_id) else {
        return steps;
    };
    while let Some(parent_id) = curr.parent_id {
        if parent_id == template.step_id {
            break;
        }
        steps.push(parent_id);
        match info_map.get(&parent_id) {
            Some(p) => curr = p,
            None => break,
        }
    }
    steps
}

/// (template_step_id, is_completed) pairs to persist for a step and its parent.
pub fn compute_completion_updates(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
) -> Vec<(i32, bool)> {
    let info_map = build_step_info_map(template);
    if !info_map.contains_key(&step_id) {
        return vec![];
    }

    let target_completed = check_step_completed(step_id, &info_map, state);

    cascade_completion_one_level(template, state, &info_map, step_id, target_completed)
}

/// Outcome of `compute_review_updates`.
#[derive(Debug)]
pub struct ReviewUpdateResult {
    pub steps_to_update: Vec<(i32, bool)>,
    pub is_completed: bool,
    pub acted_as_designation: Option<String>,
}

/// Step updates for an admin review.
pub fn compute_review_updates(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
    is_approved: bool,
    actor: &ActorContext,
) -> Result<ReviewUpdateResult, String> {
    let statuses = compute_statuses(template, state);
    let info_map = build_step_info_map(template);

    let status = statuses
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not found", step_id))?;

    let info = info_map
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not in template", step_id))?;

    if !actor.may_review(&info.filled_by) {
        return Err(match &info.filled_by {
            crate::StepOwner::User | crate::StepOwner::Admin | crate::StepOwner::Tenant => {
                "This stage can only be reviewed by an admin".to_string()
            }
            crate::StepOwner::Designation { code } => format!(
                "This stage can only be reviewed by someone holding the '{}' designation",
                code
            ),
            crate::StepOwner::WingRepresentative => {
                "This stage can only be reviewed by an admin or a wing representative".to_string()
            }
        });
    }
    let acted_as_designation = match &info.filled_by {
        crate::StepOwner::Designation { code } if actor.designation_codes.contains(code) => {
            Some(code.clone())
        }
        crate::StepOwner::WingRepresentative => actor
            .matching_wing_representative_code()
            .map(|c| c.to_string()),
        _ => None,
    };

    // Open exclusive-owner step: block approval unless its holder actually filled it first.
    match status {
        StepStatus::PendingReview => {}
        StepStatus::Open => {
            if is_approved
                && matches!(
                    info.filled_by,
                    crate::StepOwner::Designation { .. }
                        | crate::StepOwner::WingRepresentative
                        | crate::StepOwner::Tenant
                )
                && !(actor.may_act_on(&info.filled_by)
                    && check_step_completed(step_id, &info_map, state))
            {
                return Err(
                    "This stage must be filled by its assigned owner before approval".to_string(),
                );
            }
        }
        _ => return Err("Step must be open or pending review for admin action".to_string()),
    }

    if is_approved {
        // Only propagate to the parent if the step was OPEN (not yet completed) —
        // if it was already PENDING_REVIEW, the parent was handled at submit time.
        let steps_to_update = if *status == StepStatus::Open {
            cascade_completion_one_level(template, state, &info_map, step_id, true)
        } else {
            vec![(step_id, true)]
        };
        Ok(ReviewUpdateResult {
            steps_to_update,
            is_completed: true,
            acted_as_designation,
        })
    } else {
        let steps_to_update = cascade_incomplete_to_root(template, &info_map, step_id)
            .into_iter()
            .map(|id| (id, false))
            .collect();
        Ok(ReviewUpdateResult {
            steps_to_update,
            is_completed: false,
            acted_as_designation,
        })
    }
}

/// Outcome of `compute_reopen_updates`.
#[derive(Debug)]
pub struct ReopenUpdateResult {
    pub steps_to_reset: Vec<i32>,
    pub acted_as_designation: Option<String>,
}

/// Step IDs to reset to is_completed = false when reopening a completed/pending step.
pub fn compute_reopen_updates(
    template: &crate::ProcessTemplate,
    state: &ProcessStateInput,
    step_id: i32,
    actor: &ActorContext,
) -> Result<ReopenUpdateResult, String> {
    let statuses = compute_statuses(template, state);
    let info_map = build_step_info_map(template);

    let status = statuses
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not found", step_id))?;

    let info = info_map
        .get(&step_id)
        .ok_or_else(|| format!("Step {} not in template", step_id))?;

    if !actor.may_review(&info.filled_by) {
        return Err(match &info.filled_by {
            crate::StepOwner::User | crate::StepOwner::Admin | crate::StepOwner::Tenant => {
                "This stage can only be reopened by an admin".to_string()
            }
            crate::StepOwner::Designation { code } => format!(
                "This stage can only be reopened by someone holding the '{}' designation",
                code
            ),
            crate::StepOwner::WingRepresentative => {
                "This stage can only be reopened by an admin or a wing representative".to_string()
            }
        });
    }
    let acted_as_designation = match &info.filled_by {
        crate::StepOwner::Designation { code } if actor.designation_codes.contains(code) => {
            Some(code.clone())
        }
        crate::StepOwner::WingRepresentative => actor
            .matching_wing_representative_code()
            .map(|c| c.to_string()),
        _ => None,
    };

    match status {
        StepStatus::Completed | StepStatus::PendingReview => {}
        _ => {
            return Err(
                "Step is not closed; only completed or pending review steps can be reopened"
                    .to_string(),
            )
        }
    }

    Ok(ReopenUpdateResult {
        steps_to_reset: cascade_incomplete_to_root(template, &info_map, step_id),
        acted_as_designation,
    })
}

// ==========================================
// Tests
// ==========================================

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(is_admin: bool) -> ActorContext {
        ActorContext {
            is_admin,
            is_process_owner: true,
            ..Default::default()
        }
    }

    fn declaration_field(field_id: i32) -> crate::ProcessFieldTemplate {
        crate::ProcessFieldTemplate {
            field_id,
            name: "Declaration".to_string(),
            description: None,
            r#type: crate::FieldType::Declaration,
            is_optional: false,
            requirements: None,
            requires_signature: false,
            required_signs_from_designations: Vec::new(),
            options: None,
            example_document_id: None,
        }
    }

    fn step_with(
        step_id: i32,
        filled_by: crate::StepOwner,
        is_optional: bool,
        fields: Option<Vec<crate::ProcessFieldTemplate>>,
    ) -> crate::ProcessStepTemplate {
        crate::ProcessStepTemplate {
            step_id,
            name: format!("Step {step_id}"),
            description: None,
            is_optional,
            filled_by,
            fields,
            substeps: None,
        }
    }

    fn template_of(steps: Vec<crate::ProcessStepTemplate>) -> crate::ProcessTemplate {
        crate::ProcessTemplate {
            name: "T".to_string(),
            description: None,
            step_id: 1,
            is_optional: false,
            substeps: Some(steps),
            fields: None,
            tat_working_days: None,
            filled_by: crate::StepOwner::User,
        }
    }

    #[test]
    fn declaration_on_a_user_step_is_owner_only_even_for_admin() {
        let declaration = [declaration_field(10)];
        let owner = ActorContext {
            is_process_owner: true,
            ..Default::default()
        };
        let admin = ActorContext {
            is_admin: true,
            ..Default::default()
        };
        assert!(owner.may_act_on_step(&crate::StepOwner::User, &declaration));
        assert!(!admin.may_act_on_step(&crate::StepOwner::User, &declaration));
        assert!(admin.may_act_on_step(&crate::StepOwner::User, &[]));
    }

    #[test]
    fn tenant_stage_requires_a_required_owner_declaration() {
        let tenant = || step_with(2, crate::StepOwner::Tenant, false, None);
        let declaration = |optional| {
            step_with(
                3,
                crate::StepOwner::User,
                optional,
                Some(vec![declaration_field(10)]),
            )
        };
        let plain_owner = step_with(4, crate::StepOwner::User, false, None);

        assert!(tenant_stage_lacks_owner_declaration(&template_of(vec![
            tenant()
        ])));
        assert!(tenant_stage_lacks_owner_declaration(&template_of(vec![
            tenant(),
            plain_owner
        ])));
        assert!(tenant_stage_lacks_owner_declaration(&template_of(vec![
            tenant(),
            declaration(true)
        ])));
        assert!(!tenant_stage_lacks_owner_declaration(&template_of(vec![
            tenant(),
            declaration(false)
        ])));
        assert!(!tenant_stage_lacks_owner_declaration(&template_of(vec![
            declaration(false)
        ])));
    }

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
    fn saving_a_step_with_no_fields_of_its_own_does_not_complete_it_while_substeps_are_open() {
        let template = crate::test_fixtures::tenant_onboarding_fixture();
        let state = ProcessStateInput::default();

        // Step 2 has no fields of its own — only substeps 3,4,5,6 — so an empty save
        // must not mark it completed while those substeps are still open.
        let result = validate_save(&template, &state, 2, &[], &actor(false)).unwrap();

        assert_eq!(result.new_status, StepStatus::Open);
        assert!(result.steps_to_update.contains(&(2, false)));
    }

    #[test]
    fn completing_the_last_substep_does_not_complete_a_parent_with_its_own_unfilled_field() {
        let field = |field_id: i32| crate::ProcessFieldTemplate {
            field_id,
            name: "Note".to_string(),
            description: None,
            r#type: crate::FieldType::Text,
            is_optional: false,
            requirements: None,
            requires_signature: false,
            required_signs_from_designations: Vec::new(),
            options: None,
            example_document_id: None,
        };
        let template = crate::ProcessTemplate {
            name: "Mixed step".to_string(),
            description: None,
            step_id: 1,
            is_optional: false,
            fields: None,
            tat_working_days: None,
            filled_by: crate::StepOwner::User,
            substeps: Some(vec![crate::ProcessStepTemplate {
                step_id: 2,
                name: "Parent with its own field and a substep".to_string(),
                description: None,
                is_optional: false,
                filled_by: crate::StepOwner::User,
                fields: Some(vec![field(201)]),
                substeps: Some(vec![crate::ProcessStepTemplate {
                    step_id: 3,
                    name: "Substep".to_string(),
                    description: None,
                    is_optional: false,
                    filled_by: crate::StepOwner::User,
                    fields: Some(vec![field(301)]),
                    substeps: None,
                }]),
            }]),
        };

        // Fill only the substep's field, not the parent's own field.
        let mut state = ProcessStateInput::default();
        state.fields.insert(
            301,
            FieldState {
                text_value: Some("done".to_string()),
                document_id: None,
            },
        );

        let result = validate_save(
            &template,
            &state,
            3,
            &[SaveFieldInput {
                template_field_id: 301,
                text_value: Some("done".to_string()),
                document_id: None,
            }],
            &actor(false),
        )
        .unwrap();

        assert_eq!(result.new_status, StepStatus::Completed);
        assert!(result.steps_to_update.contains(&(3, true)));
        assert!(
            result.steps_to_update.contains(&(2, false)),
            "parent must stay incomplete while its own field is unfilled, even though its only substep just completed"
        );
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

        let result = validate_save(&template, &state, 3, &fields, &actor(false)).unwrap();

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
        let result = compute_review_updates(&template, &state, 2, true, &actor(true));
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.is_completed);
        assert!(result.steps_to_update.contains(&(2, true)));

        // 2. Reject PENDING_REVIEW → step reopened
        let result = compute_review_updates(&template, &state, 2, false, &actor(true));
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(!result.is_completed);
        assert!(result.steps_to_update.contains(&(2, false)));

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
        let result = compute_review_updates(&template, &state, 2, true, &actor(true));
        assert!(
            result.is_ok(),
            "Admin should be able to approve an OPEN step"
        );
        let result = result.unwrap();
        assert!(result.is_completed);
        assert!(result.steps_to_update.contains(&(2, true)));

        // 5. Admin can also reject an OPEN step
        let result = compute_review_updates(&template, &state, 2, false, &actor(true));
        assert!(
            result.is_ok(),
            "Admin should be able to reject an OPEN step"
        );
        assert!(!result.unwrap().is_completed);

        // 6. LOCKED steps are still blocked
        let result = compute_review_updates(&template, &state, 7, true, &actor(true));
        assert!(result.is_err(), "LOCKED steps should not be reviewable");
        assert!(result.unwrap_err().contains("open or pending review"));

        // 7. COMPLETED steps are still blocked
        let result = compute_review_updates(&template, &state, 3, true, &actor(true));
        assert!(result.is_err(), "COMPLETED steps should not be reviewable");
    }

    #[test]
    fn designation_owned_stage_rejects_non_holder_even_if_global_admin() {
        let mut template = crate::test_fixtures::tenant_onboarding_fixture();
        // Step 7 ("Step 2: Meeting with Committee") is Admin-owned in the fixture;
        // repoint it to a specific designation for this test.
        template.substeps.as_mut().unwrap()[1].filled_by = crate::StepOwner::Designation {
            code: "secretary".to_string(),
        };

        let mut state = ProcessStateInput::default();
        for fid in [101, 102, 103, 105, 106, 107, 108, 109, 110] {
            state.fields.insert(
                fid,
                FieldState {
                    text_value: None,
                    document_id: Some(Uuid::new_v4()),
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
        state.reviews.insert(
            2,
            vec![ReviewState {
                is_approved: true,
                created_at: chrono::Utc::now(),
            }],
        );

        // A global admin (process.reviewer) with no matching designation is rejected.
        let global_admin = actor(true);
        let result = validate_save(
            &template,
            &state,
            7,
            &[SaveFieldInput {
                template_field_id: 111,
                text_value: Some("Discussed".to_string()),
                document_id: None,
            }],
            &global_admin,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("secretary"));

        // The designation's own holder succeeds.
        let mut designation_codes = HashSet::new();
        designation_codes.insert("secretary".to_string());
        let secretary = ActorContext {
            is_admin: true,
            designation_codes,
            ..Default::default()
        };
        let result = validate_save(
            &template,
            &state,
            7,
            &[SaveFieldInput {
                template_field_id: 111,
                text_value: Some("Discussed".to_string()),
                document_id: None,
            }],
            &secretary,
        )
        .unwrap();
        assert_eq!(result.new_status, StepStatus::Completed);
        assert_eq!(result.acted_as_designation, Some("secretary".to_string()));
    }

    #[test]
    fn open_designation_step_cannot_be_approved_before_its_holder_fills_it() {
        let mut template = crate::test_fixtures::tenant_onboarding_fixture();
        template.substeps.as_mut().unwrap()[1].filled_by = crate::StepOwner::Designation {
            code: "secretary".to_string(),
        };

        let mut state = ProcessStateInput::default();
        for fid in [101, 102, 103, 105, 106, 107, 108, 109, 110] {
            state.fields.insert(
                fid,
                FieldState {
                    text_value: None,
                    document_id: Some(Uuid::new_v4()),
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
        state.reviews.insert(
            2,
            vec![ReviewState {
                is_approved: true,
                created_at: chrono::Utc::now(),
            }],
        );

        let mut designation_codes = HashSet::new();
        designation_codes.insert("secretary".to_string());
        let secretary = ActorContext {
            is_admin: true,
            designation_codes,
            ..Default::default()
        };

        let result = compute_review_updates(&template, &state, 7, true, &secretary);
        assert!(
            result.is_err(),
            "step 7's field 111 was never filled, so it must not be approvable yet"
        );

        let result = compute_review_updates(&template, &state, 7, true, &actor(true));
        assert!(
            result.is_err(),
            "a global admin without the designation must not be able to approve it either"
        );
    }

    #[test]
    fn is_pending_on_actor_respects_designation_exclusivity() {
        let mut template = crate::test_fixtures::tenant_onboarding_fixture();
        template.substeps.as_mut().unwrap()[1].filled_by = crate::StepOwner::Designation {
            code: "secretary".to_string(),
        };

        let mut state = ProcessStateInput::default();
        for fid in [101, 102, 103, 105, 106, 107, 108, 109, 110] {
            state.fields.insert(
                fid,
                FieldState {
                    text_value: None,
                    document_id: Some(Uuid::new_v4()),
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
        state.reviews.insert(
            2,
            vec![ReviewState {
                is_approved: true,
                created_at: chrono::Utc::now(),
            }],
        );

        let global_admin = actor(true);
        assert!(
            !is_pending_on_actor(&template, &state, &global_admin),
            "a global admin without the designation must not see this as pending on them"
        );

        let mut designation_codes = HashSet::new();
        designation_codes.insert("secretary".to_string());
        let secretary = ActorContext {
            is_admin: false,
            designation_codes,
            ..Default::default()
        };
        assert!(
            is_pending_on_actor(&template, &state, &secretary),
            "the designation's own holder must see this as pending on them"
        );

        let resident = ActorContext::default();
        assert!(
            !is_pending_on_actor(&template, &state, &resident),
            "an Open Designation-owned step is never pending on a plain resident"
        );
    }
}
