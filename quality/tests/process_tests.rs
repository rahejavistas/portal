use process::{
    state_machine::{SaveFieldInput, StepStatus},
    ProcessTemplate,
};
use quality::setup_db_test_env;
use std::collections::HashMap;
use uuid::Uuid;

/// Thin test helper that wraps the flat db functions to keep tests concise.
struct TestProcess<'a> {
    conn: &'a mut sqlx::PgConnection,
    pub process_id: Uuid,
    user_id: i32,
    template: &'a ProcessTemplate,
    statuses: HashMap<i32, StepStatus>,
}

impl<'a> TestProcess<'a> {
    async fn init(
        conn: &'a mut sqlx::PgConnection,
        user_id: i32,
        template: &'a ProcessTemplate,
        ownership_id: Option<i32>,
    ) -> Self {
        let process_id = process::db::init_process(&mut *conn, user_id, template, ownership_id)
            .await
            .unwrap();
        let statuses = process::db::get_statuses(&mut *conn, process_id, template)
            .await
            .unwrap();
        TestProcess {
            conn,
            process_id,
            user_id,
            template,
            statuses,
        }
    }

    async fn reload_with_user(
        conn: &'a mut sqlx::PgConnection,
        user_id: i32,
        process_id: Uuid,
        template: &'a ProcessTemplate,
    ) -> Self {
        let statuses = process::db::get_statuses(&mut *conn, process_id, template)
            .await
            .unwrap();
        TestProcess {
            conn,
            process_id,
            user_id,
            template,
            statuses,
        }
    }

    fn status(&self, step_id: i32) -> StepStatus {
        *self.statuses.get(&step_id).unwrap()
    }

    async fn refresh(&mut self) {
        self.statuses = process::db::get_statuses(&mut *self.conn, self.process_id, self.template)
            .await
            .unwrap();
    }

    async fn save_step_fields(
        &mut self,
        step_id: i32,
        fields: &[SaveFieldInput],
        is_admin: bool,
    ) -> Result<(), String> {
        process::db::save_step(
            &mut *self.conn,
            self.process_id,
            self.template,
            step_id,
            fields,
            is_admin,
        )
        .await?;
        self.refresh().await;
        Ok(())
    }

    async fn review_step(
        &mut self,
        step_id: i32,
        admin_id: Option<i32>,
        is_approved: bool,
        notes: Option<&str>,
        flagged_step_ids: &[i32],
    ) -> Result<(), String> {
        process::db::review_step(
            &mut *self.conn,
            self.process_id,
            self.template,
            process::db::ReviewStepParams {
                step_id,
                admin_id,
                is_approved,
                notes,
                flagged_step_ids,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        self.refresh().await;
        Ok(())
    }

    async fn reopen_step(&mut self, step_id: i32, notes: Option<&str>) -> Result<(), String> {
        process::db::reopen_step(
            &mut *self.conn,
            self.process_id,
            self.template,
            step_id,
            self.user_id,
            notes,
        )
        .await?;
        self.refresh().await;
        Ok(())
    }

    async fn force_step_completion(&mut self, step_id: i32, completed: bool) {
        sqlx::query(
            "UPDATE process_steps SET is_completed = $1, \
             completed_at = CASE WHEN $1 = TRUE THEN NOW() ELSE NULL END, updated_at = NOW() \
             WHERE process_id = $2 AND template_step_id = $3",
        )
        .bind(completed)
        .bind(self.process_id)
        .bind(step_id)
        .execute(&mut *self.conn)
        .await
        .unwrap();
        self.refresh().await;
    }
}

#[tokio::test]
async fn test_process_lifecycle_integration() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    // 1. Create a dummy user
    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_process_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Load the onboarding template
    let template = ProcessTemplate::load_tenant_onboarding().unwrap();

    // 2. Initialize a process workflow in the DB and construct the executor
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    // 3. Verify initial statuses
    assert_eq!(exec.status(2), StepStatus::Open); // Step 1
    assert_eq!(exec.status(3), StepStatus::Open); // Substep 3 (Owner Docs)
    assert_eq!(exec.status(7), StepStatus::Locked); // Step 2 (Meeting)

    // 4. Save fields in Substep 3 (Owner Documents)
    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'agreement.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    let fields_submitted = vec![
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

    exec.save_step_fields(3, &fields_submitted, false)
        .await
        .unwrap();

    assert_eq!(exec.status(3), StepStatus::Completed);
    assert_eq!(exec.status(2), StepStatus::Open);

    // 5. Complete all remaining substeps for Step 1
    let substep_fields = vec![
        (
            4,
            vec![SaveFieldInput {
                template_field_id: 107,
                text_value: None,
                document_id: Some(doc_id),
            }],
        ), // Tenant KYC
        (
            5,
            vec![SaveFieldInput {
                template_field_id: 108,
                text_value: None,
                document_id: Some(doc_id),
            }],
        ), // Undertaking
        (
            6,
            vec![
                SaveFieldInput {
                    template_field_id: 109,
                    text_value: None,
                    document_id: Some(doc_id),
                },
                SaveFieldInput {
                    template_field_id: 110,
                    text_value: None,
                    document_id: Some(doc_id),
                },
            ],
        ), // Payments
    ];

    for (step_id, fields) in substep_fields {
        exec.save_step_fields(step_id, &fields, false)
            .await
            .unwrap();
    }

    // Since all substeps of Step 1 are completed and it requires review, its status must be PENDING_REVIEW!
    assert_eq!(exec.status(2), StepStatus::PendingReview);

    // 6. Admin Review: Reject the step
    exec.review_step(2, None, false, Some("Invalid documents"), &[])
        .await
        .unwrap();

    // Check that Step 1 is OPEN again
    assert_eq!(exec.status(2), StepStatus::Open);

    // Drop the old executor to release borrow on conn
    let process_id = exec.process_id;
    drop(exec);

    // 7. Admin Review: Approve the step (we complete it again first by updating DB step completion to true)

    let mut exec = TestProcess::reload_with_user(&mut conn, user_id, process_id, &template).await;
    exec.force_step_completion(2, true).await;

    exec.review_step(2, None, true, Some("Approved"), &[])
        .await
        .unwrap();

    // Check that Step 1 is COMPLETED and Step 2 is now OPEN!
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open);
}

#[tokio::test]
async fn test_admin_reopen_and_edit_lifecycle() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    // 1. Create a dummy user
    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_admin_reopen_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();

    // Initialize process workflow in the DB
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    // Complete all substeps for Step 1 (step_id 2)
    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'agreement.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    // Complete step 3 (Owner Documents)
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
    exec.save_step_fields(3, &fields_substep3, false)
        .await
        .unwrap();

    // Complete remaining substeps
    exec.save_step_fields(
        4,
        &[SaveFieldInput {
            template_field_id: 107,
            text_value: None,
            document_id: Some(doc_id),
        }],
        false,
    )
    .await
    .unwrap();
    exec.save_step_fields(
        5,
        &[SaveFieldInput {
            template_field_id: 108,
            text_value: None,
            document_id: Some(doc_id),
        }],
        false,
    )
    .await
    .unwrap();
    exec.save_step_fields(
        6,
        &[
            SaveFieldInput {
                template_field_id: 109,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 110,
                text_value: None,
                document_id: Some(doc_id),
            },
        ],
        false,
    )
    .await
    .unwrap();

    // Step 1 (step_id 2) should be in PendingReview
    assert_eq!(exec.status(2), StepStatus::PendingReview);

    // Admin approves the step
    exec.review_step(2, Some(user_id), true, Some("Approved"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open); // Next step is now open

    // Non-admin tries to edit the completed step -> should fail
    let edit_res = exec.save_step_fields(3, &fields_substep3, false).await;
    assert!(edit_res.is_err());
    assert_eq!(edit_res.err().unwrap(), "Step is not open for editing");

    // Admin edits the completed step -> should succeed and NOT reject it
    exec.save_step_fields(3, &fields_substep3, true)
        .await
        .unwrap();
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open);

    // Admin manually reopens step 3 (Owner Documents) -> should set step 3 and parent step 2 to false, and insert is_approved = false
    exec.reopen_step(3, Some("Missing proof of address details."))
        .await
        .unwrap();
    assert_eq!(exec.status(3), StepStatus::Open);
    assert_eq!(exec.status(2), StepStatus::Open); // Parent step also open now
    assert_eq!(exec.status(7), StepStatus::Locked); // Next step locked again

    // Direct database assertions: Check the review history
    let reviews: Vec<(i32, bool, Option<String>)> = sqlx::query_as(
        "SELECT ps.template_step_id, pr.is_approved, pr.notes \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 \
         ORDER BY pr.created_at ASC",
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(reviews.len(), 3);
    assert_eq!(reviews[0], (2, true, Some("Approved".to_string())));
    assert_eq!(
        reviews[1],
        (
            3,
            false,
            Some("Missing proof of address details.".to_string())
        )
    );
    assert_eq!(
        reviews[2],
        (
            2,
            false,
            Some("Missing proof of address details.".to_string())
        )
    );

    // Direct database assertions: Check completion statuses in process_steps table
    let step_completions: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT template_step_id, is_completed FROM process_steps WHERE process_id = $1 ORDER BY template_step_id ASC"
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    for (template_step_id, is_completed) in step_completions {
        match template_step_id {
            2 => assert!(!is_completed),
            3 => assert!(!is_completed),
            4 => assert!(is_completed), // remains completed
            5 => assert!(is_completed), // remains completed
            6 => assert!(is_completed), // remains completed
            _ => assert!(!is_completed),
        }
    }
}

#[tokio::test]
async fn test_invalid_process_workflows_and_transitions() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_invalid_flows_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    // Initially:
    // Step 2 is Open. Substep 3 is Open. Step 7 is Locked. Substep 8 is Locked.

    // 1. Save fields on a Locked step (step 8)
    let save_locked_res = exec.save_step_fields(8, &[], false).await;
    assert!(save_locked_res.is_err());
    assert_eq!(
        save_locked_res.err().unwrap(),
        "Step is not open for editing"
    );

    // Verify no fields are stored in the DB for step 8
    let fields_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND ps.template_step_id = 8",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fields_count, 0);

    // 2. Reopen an already Open step (step 3)
    let reopen_open_res = exec.reopen_step(3, None).await;
    assert!(reopen_open_res.is_err());
    assert_eq!(
        reopen_open_res.err().unwrap(),
        "Step is not closed; only completed or pending review steps can be reopened"
    );

    // Verify no reviews were inserted
    let reviews_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reviews_count, 0);

    // 3. Review a step that is OPEN (step 2) — this is now allowed
    let review_open_res = exec
        .review_step(2, Some(user_id), true, Some("Approved"), &[])
        .await;
    assert!(
        review_open_res.is_ok(),
        "Admin should be able to review an OPEN step"
    );

    // Verify a review was inserted
    let reviews_count_after_review: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reviews_count_after_review, 1);

    // 3b. Review a LOCKED step (step 7 is Locked) — this should still fail
    //     Reset step 2 first so step 7 stays locked
    exec.force_step_completion(2, false).await;
    let review_locked_res = exec
        .review_step(7, Some(user_id), true, Some("Bad"), &[])
        .await;
    assert!(
        review_locked_res.is_err(),
        "LOCKED steps should not be reviewable"
    );
}

#[tokio::test]
async fn test_admin_reopen_parent_step_directly() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_reopen_parent_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    // Complete all substeps for Step 1 (step_id 2)
    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'agreement.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    // Complete step 3
    exec.save_step_fields(
        3,
        &[
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
        ],
        false,
    )
    .await
    .unwrap();

    // Complete step 4, 5, 6
    exec.save_step_fields(
        4,
        &[SaveFieldInput {
            template_field_id: 107,
            text_value: None,
            document_id: Some(doc_id),
        }],
        false,
    )
    .await
    .unwrap();
    exec.save_step_fields(
        5,
        &[SaveFieldInput {
            template_field_id: 108,
            text_value: None,
            document_id: Some(doc_id),
        }],
        false,
    )
    .await
    .unwrap();
    exec.save_step_fields(
        6,
        &[
            SaveFieldInput {
                template_field_id: 109,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 110,
                text_value: None,
                document_id: Some(doc_id),
            },
        ],
        false,
    )
    .await
    .unwrap();

    // Step 2 is in PendingReview
    assert_eq!(exec.status(2), StepStatus::PendingReview);

    // Admin approves step 2
    exec.review_step(2, Some(user_id), true, Some("Approved"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open); // Next step is now open

    // Admin manually reopens step 2 directly
    exec.reopen_step(2, None).await.unwrap();

    // Step 2 is now Open (since it's not approved anymore)
    assert_eq!(exec.status(2), StepStatus::Open);
    // Next step is locked again
    assert_eq!(exec.status(7), StepStatus::Locked);

    // Direct DB verification: check the review row
    let reviews: Vec<(i32, bool, Option<String>)> = sqlx::query_as(
        "SELECT ps.template_step_id, pr.is_approved, pr.notes \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 \
         ORDER BY pr.created_at ASC",
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[0], (2, true, Some("Approved".to_string())));
    assert_eq!(reviews[1], (2, false, None::<String>));

    // Direct DB verification: parent step 2 is not completed, but substeps are still completed
    let step_completions: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT template_step_id, is_completed FROM process_steps WHERE process_id = $1 ORDER BY template_step_id ASC"
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    for (template_step_id, is_completed) in step_completions {
        match template_step_id {
            2 => assert!(!is_completed),
            3..=6 => assert!(is_completed),
            _ => assert!(!is_completed),
        }
    }
}

#[tokio::test]
async fn test_field_history_preservation() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_history_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id1 = Uuid::new_v4();
    let doc_id2 = Uuid::new_v4();
    for doc_id in &[doc_id1, doc_id2] {
        sqlx::query(
            "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
             VALUES ($1, 'doc.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
        )
        .bind(doc_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Save field 101 with doc_id1
    exec.save_step_fields(
        3,
        &[SaveFieldInput {
            template_field_id: 101,
            text_value: None,
            document_id: Some(doc_id1),
        }],
        false,
    )
    .await
    .unwrap();

    // Save field 101 again with doc_id2
    exec.save_step_fields(
        3,
        &[SaveFieldInput {
            template_field_id: 101,
            text_value: None,
            document_id: Some(doc_id2),
        }],
        false,
    )
    .await
    .unwrap();

    // Verify there are exactly 2 records in the database for this field in this process
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND pf.template_field_id = 101",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);

    // Reload the process and check that it retrieves the latest value (doc_id2)
    let process_id = exec.process_id;
    drop(exec);

    let loaded_state = process::db::load_state(&mut conn, process_id)
        .await
        .unwrap();
    let field_state = loaded_state.fields.get(&101).unwrap();
    assert_eq!(field_state.document_id, Some(doc_id2));
}

#[tokio::test]
async fn test_rejection_resubmission_approval_lifecycle() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('lifecycle_test_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id1 = Uuid::new_v4();
    let doc_id2 = Uuid::new_v4();
    for doc_id in &[doc_id1, doc_id2] {
        sqlx::query(
            "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
             VALUES ($1, 'doc.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
        )
        .bind(doc_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Step 3 (Owner Documents) is Open initially. We complete it.
    exec.save_step_fields(
        3,
        &[
            SaveFieldInput {
                template_field_id: 101,
                text_value: None,
                document_id: Some(doc_id1),
            },
            SaveFieldInput {
                template_field_id: 102,
                text_value: None,
                document_id: Some(doc_id1),
            },
            SaveFieldInput {
                template_field_id: 103,
                text_value: None,
                document_id: Some(doc_id1),
            },
            SaveFieldInput {
                template_field_id: 105,
                text_value: None,
                document_id: Some(doc_id1),
            },
            SaveFieldInput {
                template_field_id: 106,
                text_value: None,
                document_id: Some(doc_id1),
            },
        ],
        false,
    )
    .await
    .unwrap();

    // Step 4 (Tenant KYC)
    exec.save_step_fields(
        4,
        &[SaveFieldInput {
            template_field_id: 107,
            text_value: None,
            document_id: Some(doc_id1),
        }],
        false,
    )
    .await
    .unwrap();
    // Step 5 (Undertaking)
    exec.save_step_fields(
        5,
        &[SaveFieldInput {
            template_field_id: 108,
            text_value: None,
            document_id: Some(doc_id1),
        }],
        false,
    )
    .await
    .unwrap();
    // Step 6 (Payments)
    exec.save_step_fields(
        6,
        &[
            SaveFieldInput {
                template_field_id: 109,
                text_value: None,
                document_id: Some(doc_id1),
            },
            SaveFieldInput {
                template_field_id: 110,
                text_value: None,
                document_id: Some(doc_id1),
            },
        ],
        false,
    )
    .await
    .unwrap();

    // Parent Step 2 (Step 1: Onboarding Documents and Payments) is now PendingReview
    assert_eq!(exec.status(2), StepStatus::PendingReview);

    // Admin rejects Parent Step 2
    exec.review_step(
        2,
        Some(user_id),
        false,
        Some("KYC documents are blurry. Please upload again."),
        &[],
    )
    .await
    .unwrap();

    // Parent Step 2 should now be Open (meaning User Action Required)
    assert_eq!(exec.status(2), StepStatus::Open);
    // Substeps remain Completed — the user can edit any of them because the parent is Open
    assert_eq!(exec.status(3), StepStatus::Completed);

    // Verify database step completion flags after rejection
    let completions_after_reject: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT template_step_id, is_completed FROM process_steps WHERE process_id = $1 ORDER BY template_step_id ASC"
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    for (template_step_id, is_completed) in completions_after_reject {
        match template_step_id {
            2 => assert!(
                !is_completed,
                "Parent Step 2 should be incomplete after rejection"
            ),
            3..=6 => assert!(
                is_completed,
                "Substep {} should remain completed after rejection",
                template_step_id
            ),
            _ => assert!(!is_completed),
        }
    }

    // User corrects and resubmits Step 3 with doc_id2 for field 101 (Sale Agreement)
    exec.save_step_fields(
        3,
        &[SaveFieldInput {
            template_field_id: 101,
            text_value: None,
            document_id: Some(doc_id2),
        }],
        false,
    )
    .await
    .unwrap();

    // Since all sibling substeps are still Completed, saving just the corrected one
    // immediately moves Parent Step 2 back to PendingReview (Admin Action Required)
    assert_eq!(exec.status(2), StepStatus::PendingReview);
    assert_eq!(exec.status(3), StepStatus::Completed);

    // Verify database step completion flags after resubmission
    let completions_after_resubmit: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT template_step_id, is_completed FROM process_steps WHERE process_id = $1 ORDER BY template_step_id ASC"
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    for (template_step_id, is_completed) in completions_after_resubmit {
        match template_step_id {
            2..=6 => assert!(
                is_completed,
                "Step {} should be completed after resubmission",
                template_step_id
            ),
            _ => assert!(!is_completed),
        }
    }

    // Verify database field history count for field 101 is indeed 2 (original and corrected)
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND pf.template_field_id = 101",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);

    // Admin approves the resubmission
    exec.review_step(2, Some(user_id), true, Some("Looks good now!"), &[])
        .await
        .unwrap();

    // Parent Step 2 is now Completed and Next Step 7 is now Open!
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open);
}

#[tokio::test]
async fn test_process_cascade_deletion_on_user_revoke() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    // 1. Create a dummy user and flat ownership
    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('test_revoke_user@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let flat_id: i32 = sqlx::query_scalar(
        "INSERT INTO flats (wing, flat_number) VALUES ('R', '101') RETURNING flat_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let ownership_id: i32 = sqlx::query_scalar(
        "INSERT INTO flat_ownerships (flat_id, user_id, is_active) VALUES ($1, $2, TRUE) RETURNING ownership_id",
    )
    .bind(flat_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // 2. Initialize a process workflow
    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let exec = TestProcess::init(&mut conn, user_id, &template, Some(ownership_id)).await;
    let process_id = exec.process_id;
    drop(exec);
    drop(conn);

    // Ensure it exists
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM processes WHERE process_id = $1)")
            .bind(process_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(exists, "Process should exist");

    // 3. Revoke flat ownership (simulate admin revoking user flat)
    sqlx::query("DELETE FROM flat_ownerships WHERE ownership_id = $1")
        .bind(ownership_id)
        .execute(&pool)
        .await
        .unwrap();

    // 4. Verify process is cascaded (deleted)
    let exists_after: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM processes WHERE process_id = $1)")
            .bind(process_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !exists_after,
        "Process should be deleted via CASCADE when ownership is revoked"
    );
}

#[tokio::test]
async fn test_multiple_documents_upload_per_step() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('test_multi_doc_user@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    // Create two documents
    let doc1_id = Uuid::new_v4();
    let doc2_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) VALUES ($1, 'doc1.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc1_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) VALUES ($1, 'doc2.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc2_id)
    .execute(&pool)
    .await
    .unwrap();

    // Upload multiple documents to different fields in the SAME step (Step 3: Owner Docs)
    let fields = vec![
        SaveFieldInput {
            template_field_id: 101, // NOC
            text_value: None,
            document_id: Some(doc1_id),
        },
        SaveFieldInput {
            template_field_id: 102, // Index II
            text_value: None,
            document_id: Some(doc2_id),
        },
    ];

    exec.save_step_fields(3, &fields, false).await.unwrap();

    // Verify fields are saved correctly with distinct documents
    let field1_doc: Option<Uuid> = sqlx::query_scalar(
        "SELECT pf.document_id FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND pf.template_field_id = 101 \
         ORDER BY pf.updated_at DESC LIMIT 1",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let field2_doc: Option<Uuid> = sqlx::query_scalar(
        "SELECT pf.document_id FROM process_fields pf \
         JOIN process_steps ps ON pf.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND pf.template_field_id = 102 \
         ORDER BY pf.updated_at DESC LIMIT 1",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(field1_doc.unwrap(), doc1_id);
    assert_eq!(field2_doc.unwrap(), doc2_id);
}

// ==========================================================================
// COMPREHENSIVE E2E TESTS
// ==========================================================================

/// Full turn-logic lifecycle: user fills → admin reviews → user fills next → admin fills admin step → etc.
/// Verifies determine_turn at every transition point.
#[tokio::test]
async fn test_turn_logic_full_lifecycle() {
    use process::state_machine::{determine_turn, ProcessTurn};

    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('turn_test@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    // T0: Fresh process → User turn (step 1 has user fields)
    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::User);

    // T1: User fills all substeps of Step 1 → Step 1 becomes PendingReview → Admin turn
    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'test.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    // Fill substep 3 (Owner Docs)
    let owner_docs: Vec<SaveFieldInput> = vec![101, 102, 103, 105, 106]
        .into_iter()
        .map(|fid| SaveFieldInput {
            template_field_id: fid,
            text_value: None,
            document_id: Some(doc_id),
        })
        .collect();
    exec.save_step_fields(3, &owner_docs, false).await.unwrap();

    // Fill substeps 4, 5, 6
    for (step, fields) in [(4, vec![107]), (5, vec![108]), (6, vec![109, 110])] {
        let inputs: Vec<SaveFieldInput> = fields
            .into_iter()
            .map(|fid| SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(doc_id),
            })
            .collect();
        exec.save_step_fields(step, &inputs, false).await.unwrap();
    }

    assert_eq!(exec.status(2), StepStatus::PendingReview);
    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // T2: Admin approves Step 1 → Step 2 opens (admin field 111) → still Admin turn
    exec.review_step(2, Some(user_id), true, Some("All documents verified"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open);

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // T3: Admin fills Step 2 (field 111 is admin-only) → PendingReview → Admin turn
    exec.save_step_fields(
        7,
        &[SaveFieldInput {
            template_field_id: 111,
            text_value: Some("Meeting held successfully".into()),
            document_id: None,
        }],
        true,
    )
    .await
    .unwrap();
    assert_eq!(exec.status(7), StepStatus::PendingReview);

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // T4: Admin approves Step 2 → Step 3 opens (admin field 112) → Admin turn
    exec.review_step(7, Some(user_id), true, Some("Approved"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(7), StepStatus::Completed);
    assert_eq!(exec.status(8), StepStatus::Open);

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // T5: Admin fills Step 3 + approves → Step 4 opens (user fields) → User turn
    exec.save_step_fields(
        8,
        &[SaveFieldInput {
            template_field_id: 112,
            text_value: None,
            document_id: Some(doc_id),
        }],
        true,
    )
    .await
    .unwrap();
    exec.review_step(8, Some(user_id), true, Some("NOC issued"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(9), StepStatus::Open);

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::User);

    // T6: User fills Step 4 → PendingReview → Admin turn
    exec.save_step_fields(
        9,
        &[
            SaveFieldInput {
                template_field_id: 113,
                text_value: None,
                document_id: Some(doc_id),
            },
            SaveFieldInput {
                template_field_id: 114,
                text_value: None,
                document_id: Some(doc_id),
            },
        ],
        false,
    )
    .await
    .unwrap();
    assert_eq!(exec.status(9), StepStatus::PendingReview);

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // T7: Admin approves Step 4 → Step 5 opens (admin field) → Admin turn
    exec.review_step(9, Some(user_id), true, Some("Agreement verified"), &[])
        .await
        .unwrap();

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // T8: Admin fills Step 5 + approves → all done → None
    exec.save_step_fields(
        10,
        &[SaveFieldInput {
            template_field_id: 115,
            text_value: None,
            document_id: Some(doc_id),
        }],
        true,
    )
    .await
    .unwrap();
    exec.review_step(10, Some(user_id), true, Some("Shifting approved"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(10), StepStatus::Completed);

    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::None);
}

/// Test that admin revoke mid-flow correctly transitions turn back to user,
/// user resubmits, and the review note shows the actual admin note (not generic prefix).
#[tokio::test]
async fn test_revoke_resubmit_turn_and_note_content() {
    use process::state_machine::{determine_turn, ProcessTurn};

    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('revoke_turn@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'doc.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    // User completes Step 1 entirely
    for (step, fids) in [
        (3, vec![101, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let inputs: Vec<SaveFieldInput> = fids
            .into_iter()
            .map(|fid| SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(doc_id),
            })
            .collect();
        exec.save_step_fields(step, &inputs, false).await.unwrap();
    }
    assert_eq!(exec.status(2), StepStatus::PendingReview);

    // Admin approves Step 1
    exec.review_step(2, Some(user_id), true, Some("Approved"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(2), StepStatus::Completed);
    assert_eq!(exec.status(7), StepStatus::Open);

    // *** Admin now revokes Step 3 (a substep) with a real note ***
    exec.reopen_step(3, Some("Possession letter is illegible, please re-upload"))
        .await
        .unwrap();

    // Step 3 and parent step 2 should be Open again, step 7 locked again
    assert_eq!(exec.status(3), StepStatus::Open);
    assert_eq!(exec.status(2), StepStatus::Open);
    assert_eq!(exec.status(7), StepStatus::Locked);

    // Turn should be User now (step 3 has user fields)
    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::User);

    // Verify the review note is the actual admin note, NOT "Step reopened by administrator: ..."
    let reviews: Vec<(i32, bool, Option<String>)> = sqlx::query_as(
        "SELECT ps.template_step_id, pr.is_approved, pr.notes \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 ORDER BY pr.created_at ASC",
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    // Should have: approve(2), reject(3), reject(2)
    assert_eq!(reviews.len(), 3);
    assert_eq!(reviews[0], (2, true, Some("Approved".to_string())));
    // The actual note — no generic prefix!
    assert_eq!(
        reviews[1],
        (
            3,
            false,
            Some("Possession letter is illegible, please re-upload".to_string())
        )
    );
    assert_eq!(
        reviews[2],
        (
            2,
            false,
            Some("Possession letter is illegible, please re-upload".to_string())
        )
    );

    // User re-uploads and resubmits
    let new_doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'new.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(new_doc_id)
    .execute(&pool)
    .await
    .unwrap();

    exec.save_step_fields(
        3,
        &vec![101, 102, 103, 105, 106]
            .into_iter()
            .map(|fid| SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(new_doc_id),
            })
            .collect::<Vec<_>>(),
        false,
    )
    .await
    .unwrap();

    // Step 1 should be PendingReview again → Admin turn
    assert_eq!(exec.status(2), StepStatus::PendingReview);
    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);
}

/// Test that one user cannot access another user's process.
/// Test that a user can use ANY of their documents (not just ones linked to the process).
#[tokio::test]
async fn test_access_control_and_cross_document_usage() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_a: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('user_a@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let user_b: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('user_b@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();

    // User A starts a process
    let mut conn = pool.acquire().await.unwrap();
    let mut exec_a = TestProcess::init(&mut conn, user_a, &template, None).await;
    let process_a_id = exec_a.process_id;

    // A global document (not linked to any process) — any user should be able to reference it
    let doc_global = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'global_doc.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_global)
    .execute(&pool)
    .await
    .unwrap();

    // A document created by User A (not linked to any process by the DB — linkage is via process_fields)
    let doc_process = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'process_doc.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_process)
    .execute(&pool)
    .await
    .unwrap();

    // User A can use a global document (not linked to this process) in their step
    exec_a
        .save_step_fields(
            3,
            &[SaveFieldInput {
                template_field_id: 101,
                text_value: None,
                document_id: Some(doc_global),
            }],
            false,
        )
        .await
        .unwrap();

    // User A can also use another independent document
    exec_a
        .save_step_fields(
            3,
            &[SaveFieldInput {
                template_field_id: 102,
                text_value: None,
                document_id: Some(doc_process),
            }],
            false,
        )
        .await
        .unwrap();

    // Verify both saved successfully
    let state = process::db::load_state(&mut *exec_a.conn, process_a_id)
        .await
        .unwrap();
    assert_eq!(
        state.fields.get(&101).unwrap().document_id,
        Some(doc_global)
    );
    assert_eq!(
        state.fields.get(&102).unwrap().document_id,
        Some(doc_process)
    );

    // User B should NOT be able to list User A's process
    drop(exec_a);
    let mut conn2 = pool.acquire().await.unwrap();
    let b_processes = process::db::list_processes_for_user(&mut conn2, user_b)
        .await
        .unwrap();
    let b_has_a_process = b_processes.iter().any(|p| p.process_id == process_a_id);
    assert!(
        !b_has_a_process,
        "User B should not see User A's process in their list"
    );

    // User A's process info should return user_a as owner
    let info = process::db::get_process_info(&mut conn2, process_a_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(info.user_id, user_a);
    assert_ne!(info.user_id, user_b);
}

/// Test various invalid operations that should fail gracefully.
#[tokio::test]
async fn test_comprehensive_failure_cases() {
    use process::state_machine::{determine_turn, ProcessTurn};

    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('fail_test@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'fail_test.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    // 1. Cannot save to a Locked step
    let result = exec
        .save_step_fields(
            7,
            &[SaveFieldInput {
                template_field_id: 111,
                text_value: Some("Should fail".into()),
                document_id: None,
            }],
            false,
        )
        .await;
    assert!(result.is_err(), "Should not save to locked step");

    // 2. Non-admin cannot fill admin-only field (step 7's field 111 is admin-only)
    // First unlock step 7 by completing step 1
    for (step, fids) in [
        (3, vec![101, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let inputs: Vec<SaveFieldInput> = fids
            .into_iter()
            .map(|fid| SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(doc_id),
            })
            .collect();
        exec.save_step_fields(step, &inputs, false).await.unwrap();
    }
    exec.force_step_completion(2, true).await;
    exec.review_step(2, Some(user_id), true, Some("OK"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(7), StepStatus::Open);

    let result = exec
        .save_step_fields(
            7,
            &[SaveFieldInput {
                template_field_id: 111,
                text_value: Some("User trying admin field".into()),
                document_id: None,
            }],
            false,
        )
        .await;
    assert!(
        result.is_err(),
        "Non-admin should not fill admin-only field"
    );

    // 3. Cannot review a LOCKED step
    let result = exec
        .review_step(8, Some(user_id), true, Some("Bad review"), &[])
        .await;
    assert!(result.is_err(), "Cannot review a step that is Locked");

    // 4. Cannot reopen a step that is still Open
    let result = exec.reopen_step(7, Some("Bad reopen")).await;
    assert!(result.is_err(), "Cannot reopen a step that is already Open");

    // 5. Field that doesn't belong to a step
    let result = exec
        .save_step_fields(
            7,
            &[SaveFieldInput {
                template_field_id: 101, // field 101 belongs to step 3, not step 7
                text_value: None,
                document_id: Some(doc_id),
            }],
            true,
        )
        .await;
    assert!(result.is_err(), "Field 101 does not belong to step 7");

    // 6. Admin CAN fill admin field → becomes PendingReview → Admin turn
    exec.save_step_fields(
        7,
        &[SaveFieldInput {
            template_field_id: 111,
            text_value: Some("Meeting done".into()),
            document_id: None,
        }],
        true,
    )
    .await
    .unwrap();

    assert_eq!(exec.status(7), StepStatus::PendingReview);
    let state = process::db::load_state(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(determine_turn(&template, &state), ProcessTurn::Admin);

    // 7. Cannot reopen a Locked step
    let result = exec.reopen_step(8, Some("Bad reopen")).await;
    assert!(result.is_err(), "Cannot reopen a Locked step");
}

/// Test that revoke without a note stores NULL (not a generic message).
#[tokio::test]
async fn test_revoke_without_note_stores_null() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('null_note@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'x.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    // Complete and get step 1 approved
    for (step, fids) in [
        (3, vec![101, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let inputs: Vec<SaveFieldInput> = fids
            .into_iter()
            .map(|fid| SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(doc_id),
            })
            .collect();
        exec.save_step_fields(step, &inputs, false).await.unwrap();
    }
    exec.review_step(2, Some(user_id), true, Some("OK"), &[])
        .await
        .unwrap();

    // Revoke with NO note
    exec.reopen_step(3, None).await.unwrap();

    // Check DB: the rejection review should have notes = NULL
    let review_notes: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT pr.notes FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND pr.is_approved = FALSE \
         ORDER BY pr.created_at ASC",
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    // All rejection reviews from this reopen should have NULL notes
    for note in &review_notes {
        assert!(
            note.is_none(),
            "Expected NULL note for revoke without note, got: {:?}",
            note
        );
    }
}

/// Test that flagged_step_ids are persisted correctly on a review and returned
/// by get_process_reviews, and that reviews with no flags store an empty array.
#[tokio::test]
async fn test_review_flagged_step_ids() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_flagged_steps@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'flag_test.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    for (step, fids) in [
        (3, vec![101i32, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let inputs: Vec<process::state_machine::SaveFieldInput> = fids
            .into_iter()
            .map(|fid| process::state_machine::SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(doc_id),
            })
            .collect();
        exec.save_step_fields(step, &inputs, false).await.unwrap();
    }
    assert_eq!(exec.status(2), StepStatus::PendingReview);

    // Admin rejects and flags substeps 3 and 5 ("Owner Documents" and "Undertaking").
    // These are template_step_ids — same integers stored in process_steps.template_step_id.
    exec.review_step(
        2,
        Some(user_id),
        false,
        Some("Please fix owner docs and undertaking."),
        &[3, 5],
    )
    .await
    .unwrap();

    // get_process_reviews should return the flags intact
    let reviews = process::db::get_process_reviews(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();

    assert_eq!(reviews.len(), 1);
    let r = &reviews[0];
    assert!(!r.is_approved);
    assert_eq!(
        r.notes.as_deref(),
        Some("Please fix owner docs and undertaking.")
    );

    // flagged_step_ids should be exactly {3, 5} (sort to normalise order)
    let mut flagged = r.flagged_step_ids.clone();
    flagged.sort();
    assert_eq!(flagged, vec![3, 5], "flagged_step_ids mismatch");

    // Submit a second review with NO flags — should persist as empty array
    exec.force_step_completion(2, true).await;
    exec.review_step(2, Some(user_id), true, Some("All good now"), &[])
        .await
        .unwrap();

    let reviews2 = process::db::get_process_reviews(&mut *exec.conn, exec.process_id)
        .await
        .unwrap();
    assert_eq!(reviews2.len(), 2);
    let approved = reviews2.iter().find(|r| r.is_approved).unwrap();
    assert!(
        approved.flagged_step_ids.is_empty(),
        "Approval with no flags should store an empty array"
    );
}

/// Test that reopen_step always stores empty flagged_step_ids on its
/// system-generated rejection reviews (never inherits admin flags).
#[tokio::test]
async fn test_reopen_step_stores_empty_flags() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) \
         VALUES ('test_reopen_flags@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ProcessTemplate::load_tenant_onboarding().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut exec = TestProcess::init(&mut conn, user_id, &template, None).await;

    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (document_id, title, category, document_type, access_level, is_user_uploaded) \
         VALUES ($1, 'reopen_flag.pdf', 'GENERIC', 'GENERIC_RULEBOOK', 'PUBLIC', TRUE)"
    )
    .bind(doc_id)
    .execute(&pool)
    .await
    .unwrap();

    for (step, fids) in [
        (3, vec![101i32, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let inputs: Vec<process::state_machine::SaveFieldInput> = fids
            .into_iter()
            .map(|fid| process::state_machine::SaveFieldInput {
                template_field_id: fid,
                text_value: None,
                document_id: Some(doc_id),
            })
            .collect();
        exec.save_step_fields(step, &inputs, false).await.unwrap();
    }

    // Approve step 2 so it becomes Completed
    exec.review_step(2, Some(user_id), true, Some("Approved"), &[])
        .await
        .unwrap();
    assert_eq!(exec.status(2), StepStatus::Completed);

    // Reopen substep 3 — inserts system-generated rejection reviews via insert_review(&[])
    exec.reopen_step(3, Some("Please re-upload with better quality."))
        .await
        .unwrap();

    // Every rejection review from reopen must carry empty flagged_step_ids
    let flagged_arrays: Vec<Vec<i32>> = sqlx::query_scalar(
        "SELECT pr.flagged_step_ids \
         FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1 AND pr.is_approved = FALSE",
    )
    .bind(exec.process_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert!(
        !flagged_arrays.is_empty(),
        "reopen_step should have inserted rejection reviews"
    );
    for flags in &flagged_arrays {
        assert!(
            flags.is_empty(),
            "reopen_step rejection reviews must have empty flagged_step_ids, got: {:?}",
            flags
        );
    }
}
