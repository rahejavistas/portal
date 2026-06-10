use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    Locked,
    Open,
    PendingReview,
    Completed,
}

// State representations loaded from DB for the pure state machine input
#[derive(Debug, Clone)]
pub struct StepStateDb {
    pub is_completed: bool,
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

#[derive(Debug, Clone)]
pub struct ProcessStateInput {
    pub steps: HashMap<i32, StepStateDb>,
    pub fields: HashMap<i32, FieldState>,
    pub reviews: HashMap<i32, Vec<ReviewState>>, // key: template_step_id
}

#[derive(Debug, Clone)]
pub struct SaveFieldInput {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct TransitionResult {
    pub steps_to_update: Vec<(i32, bool)>, // (template_step_id, is_completed)
    pub fields_to_save: Vec<(i32, Option<String>, Option<Uuid>)>, // (template_field_id, text_value, document_id)
}

#[derive(Debug, Clone)]
pub struct ReviewResult {
    pub steps_to_update: Vec<(i32, bool)>, // (template_step_id, is_completed)
}

#[derive(Debug, Clone)]
pub struct StepInfo {
    pub step_id: i32,
    pub parent_id: Option<i32>,
    pub is_review_required: bool,
    pub is_optional: bool,
    pub fields: Vec<crate::ProcessFieldTemplate>,
    pub substep_ids: Vec<i32>,
}

// Typestate pattern structs to represent step lifecycle phases
#[derive(Debug, Clone)]
pub struct Step<State> {
    pub step_id: i32,
    pub parent_id: Option<i32>,
    pub fields: Vec<crate::ProcessFieldTemplate>,
    pub field_states: HashMap<i32, FieldState>,
    _marker: std::marker::PhantomData<State>,
}

impl<State> Step<State> {
    pub fn new(
        step_id: i32,
        parent_id: Option<i32>,
        fields: Vec<crate::ProcessFieldTemplate>,
        field_states: HashMap<i32, FieldState>,
    ) -> Self {
        Self {
            step_id,
            parent_id,
            fields,
            field_states,
            _marker: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Locked;

#[derive(Debug, Clone)]
pub struct Open;

#[derive(Debug, Clone)]
pub struct PendingReview;

#[derive(Debug, Clone)]
pub struct Completed;

// Represents the output states when transitioning an Open step after saving fields
pub enum SavedStepTransition {
    RemainOpen(Step<Open>, TransitionResult),
    ToPendingReview(Step<PendingReview>, TransitionResult),
    ToCompleted(Step<Completed>, TransitionResult),
}

// Represents the output states when transitioning a PendingReview step after an admin review
pub enum ReviewedStepTransition {
    ToOpen(Step<Open>, ReviewResult),
    ToCompleted(Step<Completed>, ReviewResult),
}

impl Step<Open> {
    /// Save field values for a step that is currently Open.
    ///
    /// Transitions:
    /// - `OPEN` -> `PENDING_REVIEW` if all required fields are filled and admin review is required.
    /// - `OPEN` -> `COMPLETED` if all required fields are filled and no review is required.
    /// - `OPEN` -> `OPEN` (remains open) if some mandatory fields are still missing.
    ///
    /// Preconditions:
    /// - Only admin users can fill fields marked as `filled_by: "admin"`.
    /// - All submitted fields are validated using their respective type applicators.
    pub fn save_fields(
        mut self,
        fields_submitted: &[SaveFieldInput],
        is_admin: bool,
        step_info_map: &HashMap<i32, StepInfo>,
        current_step_states: &HashMap<i32, bool>,
    ) -> Result<SavedStepTransition, String> {
        let mut fields_to_save = Vec::new();

        for submitted in fields_submitted {
            let field_meta = self
                .fields
                .iter()
                .find(|f| f.field_id == submitted.template_field_id)
                .ok_or_else(|| {
                    format!(
                        "Field ID {} does not belong to step {}",
                        submitted.template_field_id, self.step_id
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
            )?;

            self.field_states.insert(
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

        let fields_map: HashMap<i32, (Option<String>, Option<Uuid>)> = self
            .field_states
            .iter()
            .map(|(&k, v)| (k, (v.text_value.clone(), v.document_id)))
            .collect();
        let target_completed = check_step_completed(self.step_id, step_info_map, &fields_map);

        let mut steps_to_update = vec![(self.step_id, target_completed)];

        // Propagate completion to parent step if all sibling substeps are completed
        let step_info = step_info_map.get(&self.step_id).unwrap();
        if let Some(parent_id) = step_info.parent_id {
            if let Some(parent_info) = step_info_map.get(&parent_id) {
                if parent_id != 1 {
                    // root template step_id is 1
                    let mut all_completed = true;
                    for &sub_id in &parent_info.substep_ids {
                        let is_sub_completed = if sub_id == self.step_id {
                            target_completed
                        } else {
                            *current_step_states.get(&sub_id).unwrap_or(&false)
                        };
                        if !is_sub_completed {
                            all_completed = false;
                            break;
                        }
                    }
                    steps_to_update.push((parent_id, all_completed));
                }
            }
        }

        let transition_res = TransitionResult {
            steps_to_update,
            fields_to_save,
        };

        if target_completed {
            if step_info.is_review_required {
                Ok(SavedStepTransition::ToPendingReview(
                    Step::new(self.step_id, self.parent_id, self.fields, self.field_states),
                    transition_res,
                ))
            } else {
                Ok(SavedStepTransition::ToCompleted(
                    Step::new(self.step_id, self.parent_id, self.fields, self.field_states),
                    transition_res,
                ))
            }
        } else {
            Ok(SavedStepTransition::RemainOpen(self, transition_res))
        }
    }
}

impl Step<PendingReview> {
    /// Submit an admin review (approve or reject) for a step that is Pending Review.
    ///
    /// Transitions:
    /// - **Approval (`is_approved = true`)**: Transitions the step from `PENDING_REVIEW` to `COMPLETED`.
    /// - **Rejection (`is_approved = false`)**: Transitions the step from `PENDING_REVIEW` back to `OPEN` for changes.
    ///
    /// Conditions:
    /// - A rejected step recursively resets completion states of any parent steps so they open up again.
    pub fn review(
        self,
        is_approved: bool,
        step_info_map: &HashMap<i32, StepInfo>,
    ) -> ReviewedStepTransition {
        let mut steps_to_update = Vec::new();

        if is_approved {
            let completed_step =
                Step::new(self.step_id, self.parent_id, self.fields, self.field_states);
            ReviewedStepTransition::ToCompleted(completed_step, ReviewResult { steps_to_update })
        } else {
            steps_to_update.push((self.step_id, false));

            // Traverse upwards to reset parent steps completion status
            let step_info = step_info_map.get(&self.step_id).unwrap();
            let mut curr_info = step_info;
            while let Some(parent_id) = curr_info.parent_id {
                if parent_id == 1 {
                    break;
                }
                steps_to_update.push((parent_id, false));
                if let Some(p_info) = step_info_map.get(&parent_id) {
                    curr_info = p_info;
                } else {
                    break;
                }
            }

            let open_step = Step::new(self.step_id, self.parent_id, self.fields, self.field_states);
            ReviewedStepTransition::ToOpen(open_step, ReviewResult { steps_to_update })
        }
    }
}

// Holds step typestate wrappers
#[derive(Debug, Clone)]
pub enum StepState {
    Locked(Step<Locked>),
    Open(Step<Open>),
    PendingReview(Step<PendingReview>),
    Completed(Step<Completed>),
}

impl StepState {
    pub fn status(&self) -> StepStatus {
        match self {
            StepState::Locked(_) => StepStatus::Locked,
            StepState::Open(_) => StepStatus::Open,
            StepState::PendingReview(_) => StepStatus::PendingReview,
            StepState::Completed(_) => StepStatus::Completed,
        }
    }

    pub fn parent_id(&self) -> Option<i32> {
        match self {
            StepState::Locked(s) => s.parent_id,
            StepState::Open(s) => s.parent_id,
            StepState::PendingReview(s) => s.parent_id,
            StepState::Completed(s) => s.parent_id,
        }
    }
}

// The complete workflow containing all active steps wraped in their typestates
#[derive(Debug, Clone)]
pub struct Workflow {
    pub steps: HashMap<i32, StepState>,
}

impl Workflow {
    /// Build and wrap active workflow steps in their correct typestate wrappers using the current DB state.
    pub fn from_state(template: &crate::ProcessTemplate, state_input: &ProcessStateInput) -> Self {
        let step_info_map = build_step_info_map(template);
        let statuses = compute_statuses(template, state_input);
        let mut steps = HashMap::new();

        for (&step_id, &status) in &statuses {
            if let Some(info) = step_info_map.get(&step_id) {
                let mut field_states = HashMap::new();
                for field in &info.fields {
                    if let Some(f_state) = state_input.fields.get(&field.field_id) {
                        field_states.insert(field.field_id, f_state.clone());
                    }
                }

                let step_state = match status {
                    StepStatus::Locked => StepState::Locked(Step::new(
                        step_id,
                        info.parent_id,
                        info.fields.clone(),
                        field_states,
                    )),
                    StepStatus::Open => StepState::Open(Step::new(
                        step_id,
                        info.parent_id,
                        info.fields.clone(),
                        field_states,
                    )),
                    StepStatus::PendingReview => StepState::PendingReview(Step::new(
                        step_id,
                        info.parent_id,
                        info.fields.clone(),
                        field_states,
                    )),
                    StepStatus::Completed => StepState::Completed(Step::new(
                        step_id,
                        info.parent_id,
                        info.fields.clone(),
                        field_states,
                    )),
                };
                steps.insert(step_id, step_state);
            }
        }

        Self { steps }
    }
}

/// Helper function to build a lookup map of steps from the tree template.
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

// Recursive helper to traverse the tree template and construct step info.
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

// Check if all mandatory fields in a given step are filled.
fn check_step_completed(
    step_id: i32,
    map: &HashMap<i32, StepInfo>,
    fields_map: &HashMap<i32, (Option<String>, Option<Uuid>)>,
) -> bool {
    let info = map.get(&step_id).unwrap();

    for field in &info.fields {
        if !field.is_optional {
            if let Some(val) = fields_map.get(&field.field_id) {
                if !crate::fields::is_field_filled(&field.r#type, val.0.as_deref(), val.1) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    true
}

/// Compute the status of each step based on DB completion state and reviews.
///
/// Status rules:
/// - A step is `Locked` if its preceding step is not `Completed`.
/// - A step is `Completed` if it is completed in the DB and approved (or does not require review).
/// - A step is `PendingReview` if it is completed in the DB but does not have approval yet.
/// - A step is `Open` if it is unlocked but not yet completed.
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
            if let Some(revs) = state_input.reviews.get(&step_id) {
                let latest_review = revs.iter().max_by_key(|r| r.created_at);
                if let Some(r) = latest_review {
                    r.is_approved
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            true
        }
    };

    let root_info = step_info_map.get(&template.step_id).unwrap();
    let top_step_ids = &root_info.substep_ids;

    let mut prev_approved = true;

    for &step_id in top_step_ids {
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
                StepStatus::Completed => StepStatus::Completed,
                StepStatus::PendingReview => StepStatus::Completed,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_statuses_initial() {
        let template = crate::ProcessTemplate::load_tenant_onboarding().unwrap();
        let state = ProcessStateInput {
            steps: HashMap::new(),
            fields: HashMap::new(),
            reviews: HashMap::new(),
        };

        let statuses = compute_statuses(&template, &state);

        assert_eq!(statuses.get(&2), Some(&StepStatus::Open));
        assert_eq!(statuses.get(&3), Some(&StepStatus::Open));
        assert_eq!(statuses.get(&7), Some(&StepStatus::Locked));
        assert_eq!(statuses.get(&1), Some(&StepStatus::Open));
    }

    #[test]
    fn test_transition_save_step_and_completion() {
        let template = crate::ProcessTemplate::load_tenant_onboarding().unwrap();
        let state = ProcessStateInput {
            steps: HashMap::new(),
            fields: HashMap::new(),
            reviews: HashMap::new(),
        };

        let workflow = Workflow::from_state(&template, &state);

        // Try to save field in Locked step (Step 2 - step_id 7)
        let step7_state = workflow.steps.get(&7).unwrap();
        assert!(matches!(step7_state, StepState::Locked(_)));

        // Save fields in substep 3 (Owner Documents)
        let step3_state = workflow.steps.get(&3).unwrap();
        let step3 = match step3_state {
            StepState::Open(s) => s.clone(),
            _ => panic!("Step 3 should be open"),
        };

        let doc_id = Uuid::new_v4();
        let fields_substep3 = vec![
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

        let step_info_map = build_step_info_map(&template);
        let current_step_states: HashMap<i32, bool> = state
            .steps
            .iter()
            .map(|(&k, v)| (k, v.is_completed))
            .collect();

        let transition = step3
            .save_fields(
                &fields_substep3,
                false,
                &step_info_map,
                &current_step_states,
            )
            .unwrap();
        match transition {
            SavedStepTransition::ToCompleted(_, res) => {
                assert!(res.steps_to_update.contains(&(3, true)));
                assert!(res.steps_to_update.contains(&(2, false)));
            }
            _ => panic!("Expected transition to Completed"),
        }
    }
}
