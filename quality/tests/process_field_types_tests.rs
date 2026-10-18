//! `Declaration` and `NoDues` field types: a declaration requires a ticked agreement
//! before it's filled, and a `NoDues` field blocks completion live against the latest
//! admin-uploaded dues snapshot (`flat_dues_from_file`) — never a value the resident
//! can submit themselves, and entirely independent of the accounting module.

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

/// Seeds (or overwrites) wing F's dues snapshot for one flat, the same upsert the
/// real file-upload endpoint calls after parsing — bypasses the XLSX parsing itself,
/// which is covered separately by `handlers::dues`'s own unit tests.
async fn seed_dues_snapshot(
    pool: &sqlx::PgPool,
    flat_number: &str,
    total_due: rust_decimal::Decimal,
    overdue_amount: rust_decimal::Decimal,
) {
    seed_dues_snapshot_as_of(
        pool,
        flat_number,
        total_due,
        overdue_amount,
        chrono::NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
    )
    .await;
}

async fn seed_dues_snapshot_as_of(
    pool: &sqlx::PgPool,
    flat_number: &str,
    total_due: rust_decimal::Decimal,
    overdue_amount: rust_decimal::Decimal,
    as_of: chrono::NaiveDate,
) {
    let uploader_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('no_dues_file_uploader@vistas.com', TRUE) \
         ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING user_id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    process::dues_import::upsert_flat_dues(
        &mut conn,
        as_of,
        uploader_id,
        &[process::dues_import::FlatDueRow {
            wing: "F".to_string(),
            flat_number: flat_number.to_string(),
            total_due,
            overdue_amount,
        }],
    )
    .await
    .unwrap();
}

fn no_dues_review_gated_template() -> ProcessTemplate {
    ProcessTemplate {
        name: "No Dues Review Gated Test".to_string(),
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
            filled_by: StepOwner::User,
            substeps: None,
            fields: Some(vec![ProcessFieldTemplate {
                field_id: 301,
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

#[tokio::test]
async fn no_dues_blocks_until_the_file_marks_it_paid() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_user@vistas.com", "502").await;
    seed_dues_snapshot(&pool, "502", dec!(7500.00), dec!(0)).await;

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
        "7500 outstanding in the uploaded file must keep the stage open"
    );

    // A later upload for the same flat clearing the dues to zero.
    seed_dues_snapshot(&pool, "502", dec!(0), dec!(0)).await;

    // Nothing else ever triggers a save for a stage with no other input, so the
    // process/state read path resyncs completion live — mirrored here directly.
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    let statuses_after = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        statuses_after.get(&2),
        Some(&StepStatus::Completed),
        "re-uploading the file with zero due must unblock the stage on the next load"
    );
}

#[tokio::test]
async fn no_dues_field_cannot_be_set_by_a_submitted_value() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_tamper@vistas.com", "503").await;
    seed_dues_snapshot(&pool, "503", dec!(1200.00), dec!(0)).await;

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
    // to fake their way past the real, uploaded-file-computed outstanding amount.
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
            "a submitted '0' must never override the real, file-computed outstanding amount"
        );
    }

    let statuses = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(statuses.get(&2), Some(&StepStatus::Open));
}

#[tokio::test]
async fn dues_upload_matches_by_flat_code_and_reports_unmatched_rows() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_upload@vistas.com", "506").await;
    let uploader_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('dues_uploader_admin@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let rows = vec![
        process::dues_import::FlatDueRow {
            wing: "F".to_string(),
            flat_number: "506".to_string(),
            total_due: dec!(2500.00),
            overdue_amount: dec!(0),
        },
        process::dues_import::FlatDueRow {
            wing: "Z".to_string(),
            flat_number: "9999".to_string(),
            total_due: dec!(999.00),
            overdue_amount: dec!(0),
        },
    ];
    let summary = process::dues_import::upsert_flat_dues(
        &mut conn,
        chrono::NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
        uploader_id,
        &rows,
    )
    .await
    .unwrap();
    assert_eq!(summary.matched, 1, "only the real flat should match");
    assert_eq!(
        summary.unmatched_flat_codes,
        vec!["Z9999".to_string()],
        "a flat code with no matching flat must be reported, not silently dropped"
    );

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
    assert_eq!(statuses.get(&2), Some(&StepStatus::Open));
}

#[tokio::test]
async fn re_uploading_the_file_overwrites_the_previous_snapshot_not_accumulates() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_overwrite@vistas.com", "507").await;
    seed_dues_snapshot(&pool, "507", dec!(3000.00), dec!(0)).await;

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

    let amount = process::db::get_outstanding_dues_for_process(&mut conn, process_id)
        .await
        .unwrap();
    assert_eq!(amount, dec!(3000.00));

    // A later upload for the same flat must replace, not add to, the prior figure.
    seed_dues_snapshot(&pool, "507", dec!(500.00), dec!(500.00)).await;
    let amount = process::db::get_outstanding_dues_for_process(&mut conn, process_id)
        .await
        .unwrap();
    assert_eq!(
        amount,
        dec!(500.00),
        "a new upload must overwrite the old figure, not add to it"
    );
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::Open)
    );

    seed_dues_snapshot(&pool, "507", dec!(0), dec!(0)).await;
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::Completed)
    );
}

#[tokio::test]
async fn no_dues_is_unblocked_for_a_flat_with_no_snapshot_uploaded_yet() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_never_uploaded@vistas.com", "505").await;

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

    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::Completed),
        "a flat with no dues snapshot uploaded yet owes nothing as far as this is concerned"
    );
}

#[tokio::test]
async fn an_older_dated_upload_never_overwrites_a_newer_snapshot() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_stale_guard@vistas.com", "510").await;

    let newer = chrono::NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
    let older = chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

    seed_dues_snapshot_as_of(&pool, "510", dec!(0), dec!(0), newer).await;

    // A file uploaded out of order, dated before the one already stored, carrying
    // a stale non-zero figure - must never clobber the newer, already-cleared snapshot.
    seed_dues_snapshot_as_of(&pool, "510", dec!(5000.00), dec!(5000.00), older).await;

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

    let amount = process::db::get_outstanding_dues_for_process(&mut conn, process_id)
        .await
        .unwrap();
    assert_eq!(
        amount,
        dec!(0),
        "an older-dated upload must never overwrite a newer stored snapshot"
    );

    // The reverse must still work: a genuinely newer upload replaces the stored figure.
    let newest = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    seed_dues_snapshot_as_of(&pool, "510", dec!(750.00), dec!(0), newest).await;
    let amount = process::db::get_outstanding_dues_for_process(&mut conn, process_id)
        .await
        .unwrap();
    assert_eq!(
        amount,
        dec!(750.00),
        "a genuinely newer upload must still replace the stored figure"
    );
}

#[tokio::test]
async fn an_approved_no_dues_stage_is_never_silently_reopened_if_dues_reappear() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_review_gate@vistas.com", "511").await;
    seed_dues_snapshot(&pool, "511", dec!(0), dec!(0)).await;

    let template = no_dues_review_gated_template();
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

    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::PendingReview),
        "a User-owned top-level stage must wait for admin review even with zero dues"
    );

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
    .expect("admin approves the cleared dues stage");
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::Completed)
    );

    // Dues reappear via a later upload for the same flat - the formally approved
    // stage must never be silently reopened by a background sync.
    seed_dues_snapshot(&pool, "511", dec!(9000.00), dec!(0)).await;
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::Completed),
        "an already-approved stage must not be reopened just because dues reappeared"
    );
}

#[tokio::test]
async fn completed_at_does_not_drift_on_repeated_unchanged_syncs() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_completed_at@vistas.com", "512").await;
    seed_dues_snapshot(&pool, "512", dec!(0), dec!(0)).await;

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

    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    let first_completed_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT completed_at FROM process_steps WHERE process_id = $1 AND template_step_id = 2",
    )
    .bind(process_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Nothing changed since the last sync - a repeat must not re-stamp completed_at.
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    let second_completed_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT completed_at FROM process_steps WHERE process_id = $1 AND template_step_id = 2",
    )
    .bind(process_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        first_completed_at, second_completed_at,
        "completed_at must not drift forward on an unchanged sync"
    );
}
