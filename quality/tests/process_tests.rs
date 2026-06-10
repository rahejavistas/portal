use process::{
    db::ProcessContext,
    state_machine::{SaveFieldInput, StepState, StepStatus},
    ProcessTemplate,
};
use quality::setup_db_test_env;
use uuid::Uuid;

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
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.init().await.unwrap();

    // 3. Verify initial statuses
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Open
    ); // Step 1
    assert_eq!(
        exec.workflow.steps.get(&3).unwrap().status(),
        StepStatus::Open
    ); // Substep 3 (Owner Docs)
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Locked
    ); // Step 2 (Meeting)

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

    assert_eq!(
        exec.workflow.steps.get(&3).unwrap().status(),
        StepStatus::Completed
    );
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Open
    );

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
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::PendingReview
    );

    // 6. Admin Review: Reject the step
    exec.review_step(2, None, false, Some("Invalid documents"))
        .await
        .unwrap();

    // Check that Step 1 is OPEN again
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Open
    );

    // Drop the old executor to release borrow on conn
    let process_id = exec.process_id;
    drop(exec);

    // 7. Admin Review: Approve the step (we complete it again first by updating DB step completion to true)
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.load(process_id).await.unwrap();
    exec.update_step_completion(2, true).await.unwrap();

    exec.review_step(2, None, true, Some("Approved"))
        .await
        .unwrap();

    // Check that Step 1 is COMPLETED and Step 2 is now OPEN!
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Completed
    );
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Open
    );
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
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.init().await.unwrap();

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
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::PendingReview
    );

    // Admin approves the step
    exec.review_step(2, Some(user_id), true, Some("Approved"))
        .await
        .unwrap();
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Completed
    );
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Open
    ); // Next step is now open

    // Non-admin tries to edit the completed step -> should fail
    let edit_res = exec.save_step_fields(3, &fields_substep3, false).await;
    assert!(edit_res.is_err());
    assert_eq!(edit_res.err().unwrap(), "Step is not open for editing");

    // Admin edits the completed step -> should succeed and insert is_approved = false review,
    // which puts the step back into PendingReview (if still completed) or Open (if not).
    exec.save_step_fields(3, &fields_substep3, true)
        .await
        .unwrap();
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::PendingReview
    );
    // Since step 2 is no longer Completed, step 7 should be locked
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Locked
    );

    // Approve step 2 again
    exec.review_step(2, Some(user_id), true, Some("Approved again"))
        .await
        .unwrap();
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Completed
    );
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Open
    );

    // Admin manually reopens step 3 (Owner Documents) -> should set step 3 and parent step 2 to false, and insert is_approved = false
    exec.reopen_step(3).await.unwrap();
    assert_eq!(
        exec.workflow.steps.get(&3).unwrap().status(),
        StepStatus::Open
    );
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Open
    ); // Parent step also open now
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Locked
    ); // Next step locked again

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

    assert_eq!(reviews.len(), 6);
    assert_eq!(reviews[0], (2, true, Some("Approved".to_string())));
    assert_eq!(
        reviews[1],
        (
            3,
            false,
            Some("Substep/step edited by administrator".to_string())
        )
    );
    assert_eq!(
        reviews[2],
        (
            2,
            false,
            Some("Substep/step edited by administrator".to_string())
        )
    );
    assert_eq!(reviews[3], (2, true, Some("Approved again".to_string())));
    assert_eq!(
        reviews[4],
        (3, false, Some("Step reopened by administrator".to_string()))
    );
    assert_eq!(
        reviews[5],
        (2, false, Some("Step reopened by administrator".to_string()))
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
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.init().await.unwrap();

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
    let reopen_open_res = exec.reopen_step(3).await;
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

    // 3. Review a step that is not PendingReview (step 2 is Open)
    let review_open_res = exec
        .review_step(2, Some(user_id), true, Some("Approved"))
        .await;
    assert!(review_open_res.is_err());
    assert_eq!(
        review_open_res.err().unwrap(),
        "Step is not pending admin review"
    );

    // Verify still no reviews inserted
    let reviews_count_after_review: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM process_step_reviews pr \
         JOIN process_steps ps ON pr.step_instance_id = ps.step_instance_id \
         WHERE ps.process_id = $1",
    )
    .bind(exec.process_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reviews_count_after_review, 0);
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
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.init().await.unwrap();

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
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::PendingReview
    );

    // Admin approves step 2
    exec.review_step(2, Some(user_id), true, Some("Approved"))
        .await
        .unwrap();
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Completed
    );
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Open
    ); // Next step is now open

    // Admin manually reopens step 2 directly
    exec.reopen_step(2).await.unwrap();

    // Step 2 is now Open (since it's not approved anymore)
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Open
    );
    // Next step is locked again
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Locked
    );

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
    assert_eq!(
        reviews[1],
        (2, false, Some("Step reopened by administrator".to_string()))
    );

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
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.init().await.unwrap();

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

    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let exec2 = ctx.load(process_id).await.unwrap();
    let loaded_field = exec2.workflow.steps.get(&3).unwrap();
    let field_state = match loaded_field {
        StepState::Open(s) => s.field_states.get(&101).unwrap(),
        _ => panic!("Expected step 3 to be Open"),
    };
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
    let ctx = ProcessContext::new(&mut conn, user_id, &template);
    let mut exec = ctx.init().await.unwrap();

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
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::PendingReview
    );

    // Admin rejects Parent Step 2
    exec.review_step(
        2,
        Some(user_id),
        false,
        Some("KYC documents are blurry. Please upload again."),
    )
    .await
    .unwrap();

    // Parent Step 2 should now be Open (meaning User Action Required)
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Open
    );
    // Substep 3 remains Completed, but because Parent Step 2 is Open, the user is allowed to edit it!
    assert_eq!(
        exec.workflow.steps.get(&3).unwrap().status(),
        StepStatus::Completed
    );

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

    // Resubmitting moves Parent Step 2 back to PendingReview (meaning Admin Action Required)
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::PendingReview
    );
    assert_eq!(
        exec.workflow.steps.get(&3).unwrap().status(),
        StepStatus::Completed
    );

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
    exec.review_step(2, Some(user_id), true, Some("Looks good now!"))
        .await
        .unwrap();

    // Parent Step 2 is now Completed and Next Step 7 is now Open!
    assert_eq!(
        exec.workflow.steps.get(&2).unwrap().status(),
        StepStatus::Completed
    );
    assert_eq!(
        exec.workflow.steps.get(&7).unwrap().status(),
        StepStatus::Open
    );
}
