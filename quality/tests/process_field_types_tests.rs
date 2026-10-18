//! `Declaration` and `NoDues` field types: a declaration requires a ticked agreement
//! before it's filled, and a `NoDues` field blocks completion live against the flat's
//! actual outstanding invoices — never a value the resident can submit themselves.

use process::state_machine::{ActorContext, SaveFieldInput, StepStatus};
use process::{FieldType, ProcessFieldTemplate, ProcessStepTemplate, ProcessTemplate, StepOwner};
use quality::{
    ensure_wing_exists_for_test, quick_create_user, quick_setup_admin, setup_api_test_env,
    setup_db_test_env,
};
use rust_decimal_macros::dec;

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn create_flat_rate_plan(url: &str, admin_auth: &str, amount: i64) -> i64 {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/create",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "name": "Standard Maintenance",
        "billing_type": "FLAT_RATE",
        "config": { "amount": amount },
        "frequency": "MONTHLY"
    }))
    .expect("create billing plan failed");
    assert_eq!(resp.status(), 201);
    resp.into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap()
}

fn assign_plan(url: &str, admin_auth: &str, flat_id: i32, billing_plan_id: i64) {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id,
        "billing_plan_id": billing_plan_id
    }))
    .expect("assign billing plan failed");
    assert_eq!(resp.status(), 204);
}

fn generate_invoice(
    url: &str,
    admin_auth: &str,
    flat_id: i32,
    period_start: &str,
    period_end: &str,
    due_date: &str,
) -> serde_json::Value {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/generate",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id,
        "period_start": period_start,
        "period_end": period_end,
        "due_date": due_date
    }))
    .expect("generate invoice failed");
    assert_eq!(resp.status(), 200);
    resp.into_json().unwrap()
}

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
    billing_period_start: &str,
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
         VALUES ($1, $2, $3, $6::date, ($6::date + INTERVAL '30 days')::date, ($6::date + INTERVAL '14 days')::date, $4, $4, 0, 'UNPAID', $5) \
         RETURNING invoice_id",
    )
    .bind(format!(
        "ND-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    ))
    .bind(flat_id)
    .bind(fiscal_year_id)
    .bind(amount)
    .bind(creator_id)
    .bind(billing_period_start)
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
    let invoice_id = seed_unpaid_invoice(&pool, flat_id, dec!(7500.00), "2026-10-01").await;

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
    seed_unpaid_invoice(&pool, flat_id, dec!(1200.00), "2026-10-01").await;

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

/// Proves the integration end to end through the real accounting API — the same
/// billing-plan, invoice-generation and payment-recording calls a real admin uses
/// from the Accounts section — not a raw SQL shortcut, since that's exactly the
/// path that must actually clear a `NoDues` block in production.
#[tokio::test]
async fn no_dues_unblocks_through_the_real_accounting_payment_flow() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let user_id = quick_create_user(&env, &admin_jwt, "no_dues_real_flow@vistas.com", "G", "601");
    let (flat_id, ownership_id): (i32, i32) = sqlx::query_as(
        "SELECT f.flat_id, fo.ownership_id FROM flats f \
         JOIN flat_ownerships fo ON fo.flat_id = f.flat_id \
         WHERE fo.user_id = $1 AND fo.is_active = TRUE",
    )
    .bind(user_id)
    .fetch_one(&env.db_pool)
    .await
    .unwrap();

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-10-01",
        "2026-10-31",
        "2026-10-15",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let template = no_dues_template();
    let mut conn = env.db_pool.acquire().await.unwrap();
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
        "a freshly generated, unpaid invoice must block the stage"
    );

    let pay_resp = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id,
            "amount": 4000,
            "payment_method": "upi",
            "reference_number": "NODUES-REAL-FLOW-1",
            "payment_date": "2026-10-10"
        }))
        .expect("record payment failed");
    assert_eq!(pay_resp.status(), 200);

    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    let statuses_after = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        statuses_after.get(&2),
        Some(&StepStatus::Completed),
        "paying through the real accounting API must unblock the stage"
    );
}

#[tokio::test]
async fn no_dues_sums_every_outstanding_invoice_and_tracks_partial_payment() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_multi@vistas.com", "504").await;
    let flat_id: i32 =
        sqlx::query_scalar("SELECT flat_id FROM flat_ownerships WHERE ownership_id = $1")
            .bind(ownership_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let invoice_a = seed_unpaid_invoice(&pool, flat_id, dec!(3000.00), "2026-10-01").await;
    let invoice_b = seed_unpaid_invoice(&pool, flat_id, dec!(2000.00), "2026-11-01").await;

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
        dec!(5000.00),
        "both invoices must be summed together"
    );

    // Partially pay invoice A — still outstanding overall, just a smaller remainder.
    sqlx::query("UPDATE invoices SET amount_paid = 1000.00, status = 'PARTIALLY_PAID' WHERE invoice_id = $1")
        .bind(invoice_a)
        .execute(&pool)
        .await
        .unwrap();
    let amount = process::db::get_outstanding_dues_for_process(&mut conn, process_id)
        .await
        .unwrap();
    assert_eq!(amount, dec!(4000.00));
    process::db::sync_no_dues_step_completions(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        process::db::get_statuses(&mut conn, process_id, &template)
            .await
            .unwrap()
            .get(&2),
        Some(&StepStatus::Open),
        "a partial payment must not clear the stage while anything remains outstanding"
    );

    sqlx::query("UPDATE invoices SET amount_paid = total_amount, status = 'PAID' WHERE invoice_id IN ($1, $2)")
        .bind(invoice_a)
        .bind(invoice_b)
        .execute(&pool)
        .await
        .unwrap();
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
async fn no_dues_is_unblocked_for_a_flat_with_no_invoices_at_all() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let (user_id, ownership_id) =
        seed_user_with_flat(&pool, "no_dues_never_billed@vistas.com", "505").await;

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
        "a flat that's never been billed owes nothing, so it must not be blocked"
    );
}
