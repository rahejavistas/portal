use std::collections::HashMap;
use uuid::Uuid;

/// Connection, template, and user context used to initialize or load a workflow.
pub struct ProcessContext<'a> {
    pub conn: &'a mut sqlx::PgConnection,
    pub user_id: i32,
    pub template: &'a crate::ProcessTemplate,
    pub ownership_id: Option<i32>,
}

impl<'a> ProcessContext<'a> {
    pub fn new(
        conn: &'a mut sqlx::PgConnection,
        user_id: i32,
        template: &'a crate::ProcessTemplate,
    ) -> Self {
        Self {
            conn,
            user_id,
            template,
            ownership_id: None,
        }
    }

    pub fn with_ownership(mut self, ownership_id: Option<i32>) -> Self {
        self.ownership_id = ownership_id;
        self
    }

    /// Initialize a new process workflow instance in the DB and create all step instances based on the template.
    pub async fn init(self) -> Result<ProcessExecutor<'a>, sqlx::Error> {
        // 1. Insert process
        let process_id: Uuid = sqlx::query_scalar(
            "INSERT INTO processes (user_id, template_id, ownership_id) VALUES ($1, $2, $3) RETURNING process_id",
        )
        .bind(self.user_id)
        .bind(&self.template.template_id)
        .bind(self.ownership_id)
        .fetch_one(&mut *self.conn)
        .await?;

        // 2. Insert root step
        let root_step_instance_id: Uuid = sqlx::query_scalar(
            "INSERT INTO process_steps (process_id, template_step_id, parent_step_instance_id, is_completed) \
             VALUES ($1, $2, NULL, FALSE) \
             RETURNING step_instance_id"
        )
        .bind(process_id)
        .bind(self.template.step_id)
        .fetch_one(&mut *self.conn)
        .await?;

        // 3. Insert top-level steps and their substeps iteratively
        if let Some(ref top_steps) = self.template.substeps {
            for top_step in top_steps {
                let top_step_instance_id: Uuid = sqlx::query_scalar(
                    "INSERT INTO process_steps (process_id, template_step_id, parent_step_instance_id, is_completed) \
                     VALUES ($1, $2, $3, FALSE) \
                     RETURNING step_instance_id"
                )
                .bind(process_id)
                .bind(top_step.step_id)
                .bind(root_step_instance_id)
                .fetch_one(&mut *self.conn)
                .await?;

                if let Some(ref substeps) = top_step.substeps {
                    for substep in substeps {
                        sqlx::query(
                            "INSERT INTO process_steps (process_id, template_step_id, parent_step_instance_id, is_completed) \
                             VALUES ($1, $2, $3, FALSE)"
                        )
                        .bind(process_id)
                        .bind(substep.step_id)
                        .bind(top_step_instance_id)
                        .execute(&mut *self.conn)
                        .await?;
                    }
                }
            }
        }

        let state = load_process_state(&mut *self.conn, process_id).await?;
        let workflow = crate::state_machine::Workflow::from_state(self.template, &state);

        Ok(ProcessExecutor {
            conn: self.conn,
            process_id,
            user_id: self.user_id,
            template: self.template,
            workflow,
        })
    }

    /// Load an existing process from the database and construct the executor context.
    pub async fn load(self, process_id: Uuid) -> Result<ProcessExecutor<'a>, sqlx::Error> {
        let state = load_process_state(&mut *self.conn, process_id).await?;
        let workflow = crate::state_machine::Workflow::from_state(self.template, &state);
        Ok(ProcessExecutor {
            conn: self.conn,
            process_id,
            user_id: self.user_id,
            template: self.template,
            workflow,
        })
    }
}

/// Active execution context for an initialized process workflow.
pub struct ProcessExecutor<'a> {
    pub conn: &'a mut sqlx::PgConnection,
    pub process_id: Uuid,
    pub user_id: i32,
    pub template: &'a crate::ProcessTemplate,
    pub workflow: crate::state_machine::Workflow,
}

impl<'a> ProcessExecutor<'a> {
    /// Save fields for a step, update the workflow state, and persist changes to the database.
    pub async fn save_step_fields(
        &mut self,
        target_step_id: i32,
        fields_submitted: &[crate::state_machine::SaveFieldInput],
        is_admin: bool,
    ) -> Result<(), String> {
        let step_info_map = crate::state_machine::build_step_info_map(self.template);

        let step_state = self
            .workflow
            .steps
            .get(&target_step_id)
            .ok_or_else(|| format!("Step ID {} not found in workflow", target_step_id))?;

        let is_parent_open = if let Some(parent_id) = step_state.parent_id() {
            if let Some(parent_state) = self.workflow.steps.get(&parent_id) {
                matches!(parent_state, crate::state_machine::StepState::Open(_))
            } else {
                false
            }
        } else {
            false
        };

        let step_open = match step_state {
            crate::state_machine::StepState::Open(s) => s.clone(),
            crate::state_machine::StepState::Completed(s) if is_admin || is_parent_open => {
                crate::state_machine::Step::new(
                    s.step_id,
                    s.parent_id,
                    s.fields.clone(),
                    s.field_states.clone(),
                )
            }
            crate::state_machine::StepState::PendingReview(s) if is_admin => {
                crate::state_machine::Step::new(
                    s.step_id,
                    s.parent_id,
                    s.fields.clone(),
                    s.field_states.clone(),
                )
            }
            _ => return Err("Step is not open for editing".to_string()),
        };

        let mut current_step_states = HashMap::new();
        for (&id, s) in &self.workflow.steps {
            current_step_states.insert(
                id,
                matches!(s, crate::state_machine::StepState::Completed(_)),
            );
        }

        let transition = step_open.save_fields(
            fields_submitted,
            is_admin,
            &step_info_map,
            &current_step_states,
        )?;

        let transition_res = match transition {
            crate::state_machine::SavedStepTransition::RemainOpen(_, res) => res,
            crate::state_machine::SavedStepTransition::ToPendingReview(_, res) => res,
            crate::state_machine::SavedStepTransition::ToCompleted(_, res) => res,
        };

        save_step_fields(
            &mut *self.conn,
            self.process_id,
            target_step_id,
            &transition_res.fields_to_save,
        )
        .await
        .map_err(|e| e.to_string())?;

        update_steps_completion(
            &mut *self.conn,
            self.process_id,
            &transition_res.steps_to_update,
        )
        .await
        .map_err(|e| e.to_string())?;

        self.invalidate_parent_chain(target_step_id, &step_info_map)
            .await?;

        self.refresh_workflow().await
    }

    /// Record an admin review, update the workflow state, and persist changes to the database.
    pub async fn review_step(
        &mut self,
        target_step_id: i32,
        admin_id: Option<i32>,
        is_approved: bool,
        notes: Option<&str>,
    ) -> Result<(), String> {
        let step_info_map = crate::state_machine::build_step_info_map(self.template);

        let step_state = self
            .workflow
            .steps
            .get(&target_step_id)
            .ok_or_else(|| format!("Step ID {} not found in workflow", target_step_id))?;

        let step_pending = match step_state {
            crate::state_machine::StepState::PendingReview(s) => s.clone(),
            _ => return Err("Step is not pending admin review".to_string()),
        };

        let transition = step_pending.review(is_approved, &step_info_map);
        let review_res = match transition {
            crate::state_machine::ReviewedStepTransition::ToOpen(_, res) => res,
            crate::state_machine::ReviewedStepTransition::ToCompleted(_, res) => res,
        };

        update_steps_completion(
            &mut *self.conn,
            self.process_id,
            &review_res.steps_to_update,
        )
        .await
        .map_err(|e| e.to_string())?;

        insert_step_review(
            &mut *self.conn,
            self.process_id,
            target_step_id,
            admin_id,
            is_approved,
            notes,
        )
        .await
        .map_err(|e| e.to_string())?;

        self.refresh_workflow().await
    }

    /// Manually update step completion status in the database (e.g. to bypass review) and refresh workflow.
    pub async fn update_step_completion(
        &mut self,
        target_step_id: i32,
        is_completed: bool,
    ) -> Result<(), sqlx::Error> {
        update_steps_completion(
            &mut *self.conn,
            self.process_id,
            &[(target_step_id, is_completed)],
        )
        .await?;
        let state = load_process_state(&mut *self.conn, self.process_id).await?;
        self.workflow = crate::state_machine::Workflow::from_state(self.template, &state);
        Ok(())
    }

    /// Reopen a closed step (either Completed or PendingReview) by inserting a rejection review and resetting completion for it and its parent steps.
    pub async fn reopen_step(&mut self, target_step_id: i32) -> Result<(), String> {
        let step_info_map = crate::state_machine::build_step_info_map(self.template);

        let step_state = self
            .workflow
            .steps
            .get(&target_step_id)
            .ok_or_else(|| format!("Step ID {} not found in workflow", target_step_id))?;

        match step_state {
            crate::state_machine::StepState::Completed(_)
            | crate::state_machine::StepState::PendingReview(_) => {}
            _ => {
                return Err(
                    "Step is not closed; only completed or pending review steps can be reopened"
                        .to_string(),
                )
            }
        }

        // Gather steps to reset and insert rejection reviews
        self.invalidate_parent_chain_with_note(
            target_step_id,
            &step_info_map,
            "Step reopened by administrator",
        )
        .await?;

        // Also reset the target step itself and its parents
        let mut steps_to_reset = vec![target_step_id];
        let step_info = step_info_map.get(&target_step_id).unwrap();
        let mut curr_info = step_info;
        while let Some(parent_id) = curr_info.parent_id {
            if parent_id == 1 {
                break;
            }
            steps_to_reset.push(parent_id);
            if let Some(p_info) = step_info_map.get(&parent_id) {
                curr_info = p_info;
            } else {
                break;
            }
        }

        let steps_to_update: Vec<(i32, bool)> =
            steps_to_reset.iter().map(|&id| (id, false)).collect();
        update_steps_completion(&mut *self.conn, self.process_id, &steps_to_update)
            .await
            .map_err(|e| e.to_string())?;

        self.refresh_workflow().await
    }

    // ---- Private helpers ----

    /// Reload workflow state from the database.
    async fn refresh_workflow(&mut self) -> Result<(), String> {
        let state = load_process_state(&mut *self.conn, self.process_id)
            .await
            .map_err(|e| e.to_string())?;
        self.workflow = crate::state_machine::Workflow::from_state(self.template, &state);
        Ok(())
    }

    /// Insert rejection reviews for a step and all its closed parent steps (used when admin edits a completed step).
    async fn invalidate_parent_chain(
        &mut self,
        target_step_id: i32,
        step_info_map: &HashMap<i32, crate::state_machine::StepInfo>,
    ) -> Result<(), String> {
        self.invalidate_parent_chain_with_note(
            target_step_id,
            step_info_map,
            "Substep/step edited by administrator",
        )
        .await
    }

    /// Insert rejection reviews for a step and all its closed parent steps with a custom note.
    async fn invalidate_parent_chain_with_note(
        &mut self,
        target_step_id: i32,
        step_info_map: &HashMap<i32, crate::state_machine::StepInfo>,
        note: &str,
    ) -> Result<(), String> {
        let mut steps_to_reset = vec![target_step_id];
        let mut curr_info = step_info_map.get(&target_step_id).unwrap();
        while let Some(parent_id) = curr_info.parent_id {
            if parent_id == 1 {
                break;
            }
            steps_to_reset.push(parent_id);
            if let Some(p_info) = step_info_map.get(&parent_id) {
                curr_info = p_info;
            } else {
                break;
            }
        }

        for step_id in steps_to_reset {
            let state_of_step = self.workflow.steps.get(&step_id).unwrap();
            let is_step_closed = matches!(
                state_of_step,
                crate::state_machine::StepState::Completed(_)
                    | crate::state_machine::StepState::PendingReview(_)
            );
            if is_step_closed {
                insert_step_review(
                    &mut *self.conn,
                    self.process_id,
                    step_id,
                    Some(self.user_id),
                    false,
                    Some(note),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        }

        Ok(())
    }
}

// ==========================================
// Private Database Helper Functions
// ==========================================

async fn load_process_state(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<crate::state_machine::ProcessStateInput, sqlx::Error> {
    // Fetch steps
    let steps_rows: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT template_step_id, is_completed FROM process_steps WHERE process_id = $1",
    )
    .bind(process_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut steps = HashMap::new();
    for (template_step_id, is_completed) in steps_rows {
        steps.insert(
            template_step_id,
            crate::state_machine::StepStateDb { is_completed },
        );
    }

    // Fetch fields
    let fields_rows: Vec<(i32, Option<String>, Option<Uuid>)> = sqlx::query_as(
        "SELECT DISTINCT ON (pf.template_field_id) pf.template_field_id, pf.text_value, pf.document_id \
         FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 \
         ORDER BY pf.template_field_id, pf.updated_at DESC, pf.field_instance_id DESC"
    )
    .bind(process_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut fields = HashMap::new();
    for (template_field_id, text_value, document_id) in fields_rows {
        fields.insert(
            template_field_id,
            crate::state_machine::FieldState {
                text_value,
                document_id,
            },
        );
    }

    // Fetch reviews
    let reviews_rows: Vec<(i32, bool, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT ps.template_step_id, pr.is_approved, pr.created_at \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1",
    )
    .bind(process_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut reviews: HashMap<i32, Vec<crate::state_machine::ReviewState>> = HashMap::new();
    for (template_step_id, is_approved, created_at) in reviews_rows {
        reviews
            .entry(template_step_id)
            .or_default()
            .push(crate::state_machine::ReviewState {
                is_approved,
                created_at,
            });
    }

    Ok(crate::state_machine::ProcessStateInput {
        steps,
        fields,
        reviews,
    })
}

async fn save_step_fields(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: i32,
    fields: &[(i32, Option<String>, Option<Uuid>)],
) -> Result<(), sqlx::Error> {
    let step_instance_id: Uuid = sqlx::query_scalar(
        "SELECT step_instance_id FROM process_steps WHERE process_id = $1 AND template_step_id = $2"
    )
    .bind(process_id)
    .bind(template_step_id)
    .fetch_one(&mut *conn)
    .await?;

    for &(template_field_id, ref text_value, document_id) in fields {
        sqlx::query(
            "INSERT INTO process_fields (step_instance_id, template_field_id, text_value, document_id, updated_at) \
             VALUES ($1, $2, $3, $4, NOW())"
        )
        .bind(step_instance_id)
        .bind(template_field_id)
        .bind(text_value)
        .bind(document_id)
        .execute(&mut *conn)
        .await?;
    }

    Ok(())
}

async fn update_steps_completion(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    steps_to_update: &[(i32, bool)],
) -> Result<(), sqlx::Error> {
    for &(template_step_id, is_completed) in steps_to_update {
        sqlx::query(
            "UPDATE process_steps \
             SET is_completed = $1, completed_at = CASE WHEN $1 = TRUE THEN NOW() ELSE NULL END, updated_at = NOW() \
             WHERE process_id = $2 AND template_step_id = $3"
        )
        .bind(is_completed)
        .bind(process_id)
        .bind(template_step_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn insert_step_review(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
    template_step_id: i32,
    admin_id: Option<i32>,
    is_approved: bool,
    notes: Option<&str>,
) -> Result<(), sqlx::Error> {
    let step_instance_id: Uuid = sqlx::query_scalar(
        "SELECT step_instance_id FROM process_steps WHERE process_id = $1 AND template_step_id = $2"
    )
    .bind(process_id)
    .bind(template_step_id)
    .fetch_one(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO process_step_reviews (step_instance_id, admin_id, is_approved, notes, created_at) \
         VALUES ($1, $2, $3, $4, NOW())"
    )
    .bind(step_instance_id)
    .bind(admin_id)
    .bind(is_approved)
    .bind(notes)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct ProcessReviewHistory {
    pub review_id: Uuid,
    pub template_step_id: i32,
    pub admin_email: Option<String>,
    pub is_approved: bool,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
pub struct ProcessFieldHistoryRow {
    pub template_field_id: i32,
    pub text_value: Option<String>,
    pub document_id: Option<Uuid>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProcessInfo {
    pub user_id: i32,
    pub template_id: String,
    pub ownership_id: Option<i32>,
}

pub async fn get_process_info(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Option<ProcessInfo>, sqlx::Error> {
    sqlx::query_as("SELECT user_id, template_id, ownership_id FROM processes WHERE process_id = $1")
        .bind(process_id)
        .fetch_optional(conn)
        .await
}

pub async fn find_process_by_ownership_and_template(
    conn: &mut sqlx::PgConnection,
    ownership_id: Option<i32>,
    user_id: i32,
    template_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    if let Some(oid) = ownership_id {
        sqlx::query_scalar(
            "SELECT process_id FROM processes WHERE ownership_id = $1 AND template_id = $2",
        )
        .bind(oid)
        .bind(template_id)
        .fetch_optional(conn)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT process_id FROM processes WHERE user_id = $1 AND template_id = $2 AND ownership_id IS NULL",
        )
        .bind(user_id)
        .bind(template_id)
        .fetch_optional(conn)
        .await
    }
}

pub async fn get_process_reviews(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<ProcessReviewHistory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT pr.review_id, ps.template_step_id, u.email as admin_email, pr.is_approved, pr.notes, pr.created_at \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         LEFT JOIN users u ON pr.admin_id = u.user_id \
         WHERE ps.process_id = $1 \
         ORDER BY pr.created_at ASC"
    )
    .bind(process_id)
    .fetch_all(conn)
    .await
}

pub async fn get_field_history(
    conn: &mut sqlx::PgConnection,
    process_id: Uuid,
) -> Result<Vec<ProcessFieldHistoryRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT pf.template_field_id, pf.text_value, pf.document_id, pf.updated_at \
         FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 \
         ORDER BY pf.template_field_id, pf.updated_at DESC, pf.field_instance_id DESC",
    )
    .bind(process_id)
    .fetch_all(conn)
    .await
}

/// Fetch all active flat ownerships for a user as (ownership_id, flat_id) pairs.
pub async fn get_active_ownerships_for_user(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
) -> Result<Vec<(i32, i32)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT ownership_id, flat_id FROM flat_ownerships WHERE user_id = $1 AND is_active = TRUE",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await
}
