//! `Declaration` and `NoDues` field types: a declaration requires a ticked agreement
//! before it's filled, and a `NoDues` field blocks completion live against the flat's
//! actual outstanding invoices — never a value the resident can submit themselves.

use process::state_machine::{ActorContext, SaveFieldInput, StepStatus};
use process::{FieldType, ProcessFieldTemplate, ProcessStepTemplate, ProcessTemplate, StepOwner};
use quality::{ensure_wing_exists_for_test, setup_db_test_env};
use rust_decimal_macros::dec;

fn actor(is_admin: bool) -> ActorContext {
    ActorContext {
        is_admin,
        designation_codes: Default::default(),
    }
}

async fn seed_template_version(conn: &mut sqlx::PgConnection, template: &ProcessTemplate) -> i32 {
    let template_id: i32 = sqlx::query_scalar(
        "INSERT INTO process_templates (name) VALUES ($1) RETURNING template_id",
    )
    .bind(&template.name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let definition = serde_json::to_value(template).unwrap();
    sqlx::query_scalar(
        "INSERT INTO process_template_versions (template_id, version_number, definition) \
         VALUES ($1, 1, $2) RETURNING version_id",
    )
    .bind(template_id)
    .bind(&definition)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

async fn seed_user_with_flat(pool: &sqlx::PgPool, email: &str, flat_number: &str) -> (i32, i32) {
    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ($1, TRUE) RETURNING user_id",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .unwrap();
    ensure_wing_exists_for_test(pool, "F").await;
    let flat_id: i32 = sqlx::query_scalar(
        "INSERT INTO flats (wing, flat_number) VALUES ('F', $1) RETURNING flat_id",
    )
    .bind(flat_number)
    .fetch_one(pool)
    .await
    .unwrap();
    let ownership_id: i32 = sqlx::query_scalar(
        "INSERT INTO flat_ownerships (flat_id, user_id, is_active) VALUES ($1, $2, TRUE) RETURNING ownership_id",
    )
    .bind(flat_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap();
    (user_id, ownership_id)
}

fn declaration_template() -> ProcessTemplate {
    ProcessTemplate {
        name: "Declaration Field Test".to_string(),
        description: None,
        step_id: 1,
        is_optional: false,
        fields: None,
        tat_working_days: None,
        filled_by: StepOwner::User,
        substeps: Some(vec![ProcessStepTemplate {
            step_id: 2,
            name: "Declaration Stage".to_string(),
            description: None,
            is_optional: false,
            filled_by: StepOwner::User,
            substeps: None,
            fields: Some(vec![ProcessFieldTemplate {
                field_id: 101,
                name: "Owner Declaration".to_string(),
                description: None,
                r#type: FieldType::Declaration,
                is_optional: false,
                requirements: Some(serde_json::json!({
                    "declaration_text": "I, ##NAME##, hereby declare this is accurate."
                })),
                requires_signature: false,
                required_signs_from_designations: vec![],
                options: None,
                example_document_id: None,
            }]),
        }]),
    }
}

#[tokio::test]
async fn declaration_requires_agreement_before_the_step_completes() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "declaration_user@vistas.com", "501").await;
    let template = declaration_template();
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(
        &mut conn,
        user_id,
        version_id,
        &template,
        Some(ownership_id),
    )
    .await
    .unwrap()
    .process_id;

    let not_agreed = r#"{"agreed":false,"blanks":{"NAME":"Jane Doe"}}"#;
    let result = process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 101,
            text_value: Some(not_agreed.to_string()),
            document_id: None,
        }],
        &actor(false),
        user_id,
    )
    .await;
    let Err(err) = result else {
        panic!("an unagreed declaration must not be accepted as a complete save");
    };
    assert!(
        err.contains("mandatory"),
        "an unagreed declaration must not satisfy a non-optional field: {err}"
    );

    let agreed = r#"{"agreed":true,"blanks":{"NAME":"Jane Doe"}}"#;
    let outcome = process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 101,
            text_value: Some(agreed.to_string()),
            document_id: None,
        }],
        &actor(false),
        user_id,
    )
    .await
    .expect("an agreed, filled-in declaration must be accepted");
    // Top-level User-owned stages always need admin review before completing —
    // that's unrelated to Declaration itself, same as any other field type here.
    assert_eq!(outcome.status, StepStatus::PendingReview);

    process::db::review_step(
        &mut conn,
        process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: 2,
            reviewer_user_id: Some(user_id),
            is_approved: true,
            notes: None,
            flagged_step_ids: &[],
            acted_as_designation: None,
        },
        &actor(true),
    )
    .await
    .expect("admin approves the agreed declaration");
    let statuses = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(statuses.get(&2), Some(&StepStatus::Completed));
}

fn no_dues_template() -> ProcessTemplate {
    ProcessTemplate {
        name: "No Dues Field Test".to_string(),
        description: None,
        step_id: 1,
        is_optional: false,
        fields: None,
        tat_working_days: None,
        filled_by: StepOwner::User,
        substeps: Some(vec![ProcessStepTemplate {
            step_id: 2,
            name: "Dues Check Stage".to_string(),
            description: None,
            is_optional: false,
            filled_by: StepOwner::Admin,
            substeps: None,
            fields: Some(vec![ProcessFieldTemplate {
                field_id: 201,
                name: "No Dues".to_string(),
                description: None,
                r#type: FieldType::NoDues,
                is_optional: false,
                requirements: None,
                requires_signature: false,
                required_signs_from_designations: vec![],
                options: None,
                example_document_id: None,
            }]),
        }]),
    }
}

async fn seed_unpaid_invoice(
    pool: &sqlx::PgPool,
    flat_id: i32,
    amount: rust_decimal::Decimal,
) -> uuid::Uuid {
    let creator_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('no_dues_billing_admin@vistas.com', TRUE) \
         ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING user_id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    // The seed migration already inserts the current fiscal year; reuse it rather
    // than risk colliding with it on the (start_date, end_date) unique constraint.
    let fiscal_year_id: i32 = sqlx::query_scalar(
        "SELECT fiscal_year_id FROM fiscal_years ORDER BY start_date DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query_scalar(
        "INSERT INTO invoices (invoice_number, flat_id, fiscal_year_id, billing_period_start, \
                                billing_period_end, due_date, subtotal, total_amount, amount_paid, \
                                status, created_by) \
         VALUES ($1, $2, $3, '2026-10-01', '2026-10-31', '2026-10-15', $4, $4, 0, 'UNPAID', $5) \
         RETURNING invoice_id",
    )
    .bind(format!("NODUES-TEST-{}", flat_id))
    .bind(flat_id)
    .bind(fiscal_year_id)
    .bind(amount)
    .bind(creator_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn no_dues_blocks_until_the_invoice_is_actually_paid() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_user@vistas.com", "502").await;
    let flat_id: i32 =
        sqlx::query_scalar("SELECT flat_id FROM flat_ownerships WHERE ownership_id = $1")
            .bind(ownership_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let invoice_id = seed_unpaid_invoice(&pool, flat_id, dec!(7500.00)).await;

    let template = no_dues_template();
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(
        &mut conn,
        user_id,
        version_id,
        &template,
        Some(ownership_id),
    )
    .await
    .unwrap()
    .process_id;

    let statuses = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        statuses.get(&2),
        Some(&StepStatus::Open),
        "7500 outstanding must keep the stage open, even with nothing else to fill"
    );

    sqlx::query(
        "UPDATE invoices SET amount_paid = total_amount, status = 'PAID' WHERE invoice_id = $1",
    )
    .bind(invoice_id)
    .execute(&pool)
    .await
    .unwrap();

    // Nothing else ever triggers a save for a stage with no other input, so the
    // process/state read path resyncs completion live — mirrored here directly.
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    let statuses_after_payment = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        statuses_after_payment.get(&2),
        Some(&StepStatus::Completed),
        "paying off the invoice must unblock the stage on the very next load, with no resident action"
    );
}

#[tokio::test]
async fn no_dues_field_cannot_be_set_by_a_submitted_value() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_tamper@vistas.com", "503").await;
    let flat_id: i32 =
        sqlx::query_scalar("SELECT flat_id FROM flat_ownerships WHERE ownership_id = $1")
            .bind(ownership_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    seed_unpaid_invoice(&pool, flat_id, dec!(1200.00)).await;

    let template = no_dues_template();
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(
        &mut conn,
        user_id,
        version_id,
        &template,
        Some(ownership_id),
    )
    .await
    .unwrap()
    .process_id;

    // Even an actor who legitimately owns this Admin-owned stage must not be able
    // to fake their way past the real, live-computed outstanding amount.
    let err = process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("0".to_string()),
            document_id: None,
        }],
        &actor(true),
        user_id,
    )
    .await;
    if let Ok(outcome) = err {
        assert_ne!(
            outcome.status,
            StepStatus::Completed,
            "a submitted '0' must never override the real, live-computed outstanding amount"
        );
    }

    let statuses = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(statuses.get(&2), Some(&StepStatus::Open));
}
