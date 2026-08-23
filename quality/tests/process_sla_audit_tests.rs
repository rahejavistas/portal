//! SLA/TAT working-day clock and the append-only process audit log: clock start on
//! full submission, pause on return, resume on resubmission, and one audit row per
//! status change / verification flag / return reason.

use process::{
    state_machine::SaveFieldInput, FieldType, ProcessFieldTemplate, ProcessStepTemplate,
    ProcessTemplate,
};
use quality::setup_db_test_env;

fn sla_template(tat_working_days: Option<i32>) -> ProcessTemplate {
    ProcessTemplate {
        name: "SLA Test Process".to_string(),
        description: None,
        step_id: 1,
        is_optional: false,
        is_review_required: false,
        tat_working_days,
        fields: None,
        substeps: Some(vec![ProcessStepTemplate {
            step_id: 2,
            name: "Only step".to_string(),
            description: None,
            is_optional: false,
            is_review_required: true,
            fields: Some(vec![ProcessFieldTemplate {
                field_id: 201,
                name: "Note".to_string(),
                r#type: FieldType::Text,
                is_optional: false,
                filled_by: "user".to_string(),
                requirements: None,
                requires_signature: false,
                required_signs_from_designations: Vec::new(),
                options: None,
            }]),
            substeps: None,
        }]),
    }
}

async fn seed_user(pool: &sqlx::PgPool, email: &str) -> i32 {
    sqlx::query_scalar("INSERT INTO users (email, is_active) VALUES ($1, TRUE) RETURNING user_id")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
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
    .fetch_one(conn)
    .await
    .unwrap()
}

#[derive(sqlx::FromRow)]
struct SlaRow {
    tat_working_days: Option<i32>,
    sla_started_at: Option<chrono::DateTime<chrono::Utc>>,
    sla_due_date: Option<chrono::NaiveDate>,
    sla_paused_at: Option<chrono::DateTime<chrono::Utc>>,
    sla_paused_working_days: i32,
}

async fn load_sla_row(conn: &mut sqlx::PgConnection, process_id: uuid::Uuid) -> SlaRow {
    sqlx::query_as(
        "SELECT tat_working_days, sla_started_at, sla_due_date, sla_paused_at, sla_paused_working_days \
         FROM processes WHERE process_id = $1",
    )
    .bind(process_id)
    .fetch_one(conn)
    .await
    .unwrap()
}

#[tokio::test]
async fn test_sla_clock_stays_off_without_tat_working_days() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "sla_off@vistas.test").await;
    let template = sla_template(None);
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("done".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    let row = load_sla_row(&mut conn, process_id).await;
    assert!(
        row.sla_due_date.is_none(),
        "no tat_working_days means no SLA tracking"
    );
    assert!(row.sla_started_at.is_none());
}

#[tokio::test]
async fn test_sla_clock_starts_on_first_full_submission_not_on_process_start() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "sla_start@vistas.test").await;
    let template = sla_template(Some(3));
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    let before = load_sla_row(&mut conn, process_id).await;
    assert!(
        before.sla_due_date.is_none(),
        "clock must not start at process creation"
    );

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("done".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    let after = load_sla_row(&mut conn, process_id).await;
    assert_eq!(after.tat_working_days, Some(3));
    assert!(after.sla_started_at.is_some());
    assert!(after.sla_due_date.is_some());

    let audit = process::audit::get_audit_log(&mut conn, process_id)
        .await
        .unwrap();
    assert!(
        audit
            .iter()
            .any(|e| e.event_type == "STEP_COMPLETED" && e.template_step_id == Some(2)),
        "submitting the step must append a STEP_COMPLETED audit row"
    );
}

#[tokio::test]
async fn test_sla_clock_pauses_on_return_and_resumes_on_resubmission() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "sla_pause_user@vistas.test").await;
    let admin_id = seed_user(&pool, "sla_pause_admin@vistas.test").await;
    let template = sla_template(Some(5));
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("first submission".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();
    assert!(load_sla_row(&mut conn, process_id)
        .await
        .sla_due_date
        .is_some());

    process::db::review_step(
        &mut conn,
        process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: 2,
            reviewer_user_id: Some(admin_id),
            is_approved: false,
            notes: Some("Please fix the note."),
            flagged_step_ids: &[2],
        },
    )
    .await
    .unwrap();

    let paused = load_sla_row(&mut conn, process_id).await;
    assert!(
        paused.sla_paused_at.is_some(),
        "a return must pause the clock"
    );

    let audit = process::audit::get_audit_log(&mut conn, process_id)
        .await
        .unwrap();
    assert!(
        audit.iter().any(|e| e.event_type == "STEP_RETURNED"
            && e.detail.as_deref() == Some("Please fix the note.")),
        "the return reason must be in the audit trail"
    );
    assert!(
        audit
            .iter()
            .any(|e| e.event_type == "STEP_FLAGGED" && e.template_step_id == Some(2)),
        "the verification flag must be in the audit trail"
    );

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("fixed submission".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    let resumed = load_sla_row(&mut conn, process_id).await;
    assert!(
        resumed.sla_paused_at.is_none(),
        "resubmission must resume the clock"
    );
    assert!(resumed.sla_paused_working_days >= 0);
}

#[tokio::test]
async fn test_sla_due_date_honors_configured_public_holidays() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "sla_holiday@vistas.test").await;
    let template = sla_template(Some(1));
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    let holidays_before = process::db::get_holidays(&mut conn).await.unwrap();
    let today = chrono::Utc::now().date_naive();
    let due_without_holiday = process::working_days::add_working_days(today, 1, &holidays_before);

    process::db::add_public_holiday(&mut conn, due_without_holiday, "Test Holiday")
        .await
        .unwrap();

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("done".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    let holidays_after = process::db::get_holidays(&mut conn).await.unwrap();
    let expected_due = process::working_days::add_working_days(today, 1, &holidays_after);
    assert_ne!(
        expected_due, due_without_holiday,
        "the added holiday must actually push the due date further out"
    );

    let row = load_sla_row(&mut conn, process_id).await;
    assert_eq!(row.sla_due_date, Some(expected_due));
}

#[tokio::test]
async fn test_step_approval_is_audited() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "sla_approve_user@vistas.test").await;
    let admin_id = seed_user(&pool, "sla_approve_admin@vistas.test").await;
    let template = sla_template(None);
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("done".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    process::db::review_step(
        &mut conn,
        process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: 2,
            reviewer_user_id: Some(admin_id),
            is_approved: true,
            notes: None,
            flagged_step_ids: &[],
        },
    )
    .await
    .unwrap();

    let audit = process::audit::get_audit_log(&mut conn, process_id)
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|e| e.event_type == "STEP_APPROVED" && e.template_step_id == Some(2)));
}

#[tokio::test]
async fn test_process_closing_appends_one_process_closed_event_and_computes_tat_actual() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "tat_actual_user@vistas.test").await;
    let admin_id = seed_user(&pool, "tat_actual_admin@vistas.test").await;
    let template = sla_template(Some(4));
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    assert_eq!(
        process::db::compute_tat_actual(&mut conn, process_id)
            .await
            .unwrap(),
        None,
        "no SLA started yet, so no TATActual"
    );

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("done".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    assert_eq!(
        process::db::compute_tat_actual(&mut conn, process_id)
            .await
            .unwrap(),
        None,
        "SLA started but the process hasn't closed yet"
    );

    process::db::review_step(
        &mut conn,
        process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: 2,
            reviewer_user_id: Some(admin_id),
            is_approved: true,
            notes: None,
            flagged_step_ids: &[],
        },
    )
    .await
    .unwrap();

    let audit = process::audit::get_audit_log(&mut conn, process_id)
        .await
        .unwrap();
    let closed_events: Vec<_> = audit
        .iter()
        .filter(|e| e.event_type == "PROCESS_CLOSED")
        .collect();
    assert_eq!(
        closed_events.len(),
        1,
        "closing a process must append exactly one PROCESS_CLOSED row"
    );

    let tat_actual = process::db::compute_tat_actual(&mut conn, process_id)
        .await
        .unwrap();
    assert_eq!(
        tat_actual,
        Some(0),
        "start and close land on the same working day in this test, with no pauses"
    );
}

#[tokio::test]
async fn test_tat_actual_summary_groups_by_template() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "tat_summary_user@vistas.test").await;
    let admin_id = seed_user(&pool, "tat_summary_admin@vistas.test").await;
    let template = sla_template(Some(2));
    let version_id = seed_template_version(&mut conn, &template).await;

    for i in 0..2 {
        let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
            .await
            .unwrap();
        process::db::save_step(
            &mut conn,
            process_id,
            &template,
            2,
            &[SaveFieldInput {
                template_field_id: 201,
                text_value: Some(format!("run {i}")),
                document_id: None,
            }],
            false,
            user_id,
        )
        .await
        .unwrap();
        process::db::review_step(
            &mut conn,
            process_id,
            &template,
            process::db::ReviewStepParams {
                step_id: 2,
                reviewer_user_id: Some(admin_id),
                is_approved: true,
                notes: None,
                flagged_step_ids: &[],
            },
        )
        .await
        .unwrap();
    }

    let summary = process::db::tat_actual_summary_by_template(&mut conn)
        .await
        .unwrap();
    let row = summary
        .iter()
        .find(|s| s.template_name == "SLA Test Process")
        .expect("closed processes for this template must appear in the summary");
    assert_eq!(row.closed_count, 2);
    assert_eq!(row.average_tat_actual_working_days, 0.0);
}

#[tokio::test]
async fn test_2026_holiday_calendar_is_seeded() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let holidays = process::db::list_public_holidays(&mut conn).await.unwrap();
    assert!(
        holidays.len() >= 23,
        "expected the full 2026 seed, got {} rows",
        holidays.len()
    );

    let republic_day = holidays
        .iter()
        .find(|h| h.holiday_date == chrono::NaiveDate::from_ymd_opt(2026, 1, 26).unwrap())
        .expect("Republic Day must be seeded");
    assert_eq!(republic_day.description, "Republic Day");

    let christmas = holidays
        .iter()
        .find(|h| h.holiday_date == chrono::NaiveDate::from_ymd_opt(2026, 12, 25).unwrap())
        .expect("Christmas must be seeded");
    assert_eq!(christmas.description, "Christmas");
}

#[tokio::test]
async fn test_field_history_records_the_actor_who_saved_it() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "field_history_actor@vistas.test").await;
    let template = sla_template(None);
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap();

    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[SaveFieldInput {
            template_field_id: 201,
            text_value: Some("first value".to_string()),
            document_id: None,
        }],
        false,
        user_id,
    )
    .await
    .unwrap();

    let history = process::db::get_field_history(&mut conn, process_id)
        .await
        .unwrap();
    let entry = history
        .iter()
        .find(|h| h.template_field_id == 201)
        .expect("field 201 must have history");
    assert_eq!(
        entry.actor_email.as_deref(),
        Some("field_history_actor@vistas.test")
    );
}
