//! InwardRegNo: auto-sequenced, human-facing register number assigned at process
//! creation, and the FIFO ordering it drives on the admin "all processes" list.

use process::{FieldType, ProcessFieldTemplate, ProcessStepTemplate, ProcessTemplate, StepOwner};
use quality::setup_db_test_env;

fn inward_reg_no_template() -> ProcessTemplate {
    ProcessTemplate {
        name: "Inward Reg No Test Process".to_string(),
        description: None,
        step_id: 1,
        is_optional: false,
        tat_working_days: None,
        fields: None,
        filled_by: StepOwner::User,
        substeps: Some(vec![ProcessStepTemplate {
            step_id: 2,
            name: "Only step".to_string(),
            description: None,
            is_optional: false,
            filled_by: StepOwner::User,
            fields: Some(vec![ProcessFieldTemplate {
                field_id: 201,
                name: "Note".to_string(),
                description: None,
                r#type: FieldType::Text,
                is_optional: false,
                requirements: None,
                requires_signature: false,
                required_signs_from_designations: Vec::new(),
                options: None,
                example_document_id: None,
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

fn expected_prefix() -> String {
    format!("INW-{}-", chrono::Utc::now().format("%Y"))
}

#[tokio::test]
async fn test_new_process_gets_well_formatted_inward_reg_no() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "inward_fmt@vistas.test").await;
    let template = inward_reg_no_template();
    let version_id = seed_template_version(&mut conn, &template).await;

    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;

    let info = process::db::get_process_info(&mut conn, process_id)
        .await
        .unwrap()
        .expect("process just created must exist");

    let prefix = expected_prefix();
    assert!(
        info.inward_reg_no.starts_with(&prefix),
        "expected inward_reg_no to start with {}, got {}",
        prefix,
        info.inward_reg_no
    );
    let seq_part = &info.inward_reg_no[prefix.len()..];
    assert_eq!(
        seq_part.len(),
        6,
        "sequence part must be zero-padded to 6 digits"
    );
    assert!(
        seq_part.chars().all(|c| c.is_ascii_digit()),
        "sequence part must be numeric, got {}",
        seq_part
    );
}

#[tokio::test]
async fn test_sequential_processes_get_increasing_inward_reg_no() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "inward_seq@vistas.test").await;
    let template = inward_reg_no_template();
    let version_id = seed_template_version(&mut conn, &template).await;

    let first_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;
    let second_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;
    let third_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;

    let first = process::db::get_process_info(&mut conn, first_id)
        .await
        .unwrap()
        .unwrap();
    let second = process::db::get_process_info(&mut conn, second_id)
        .await
        .unwrap()
        .unwrap();
    let third = process::db::get_process_info(&mut conn, third_id)
        .await
        .unwrap()
        .unwrap();

    assert_ne!(first.inward_reg_no, second.inward_reg_no);
    assert_ne!(second.inward_reg_no, third.inward_reg_no);
    assert!(first.inward_reg_no < second.inward_reg_no);
    assert!(second.inward_reg_no < third.inward_reg_no);
}

#[tokio::test]
async fn test_list_all_processes_is_ordered_by_inward_reg_no_not_created_at() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id = seed_user(&pool, "inward_fifo@vistas.test").await;
    let template = inward_reg_no_template();
    let version_id = seed_template_version(&mut conn, &template).await;

    let first_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;
    let second_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;
    let third_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;

    // Scramble created_at so it disagrees with inward_reg_no order: if list_all_processes
    // were still sorting by created_at, this ordering would come out wrong.
    sqlx::query(
        "UPDATE processes SET created_at = NOW() - INTERVAL '1 hour' WHERE process_id = $1",
    )
    .bind(third_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE processes SET created_at = NOW() - INTERVAL '2 hours' WHERE process_id = $1",
    )
    .bind(second_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE processes SET created_at = NOW() - INTERVAL '3 hours' WHERE process_id = $1",
    )
    .bind(first_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let all = process::db::list_all_processes(&mut conn).await.unwrap();
    let ours: Vec<_> = all
        .into_iter()
        .filter(|p| [first_id, second_id, third_id].contains(&p.process_id))
        .collect();

    assert_eq!(ours.len(), 3);
    assert_eq!(ours[0].process_id, first_id);
    assert_eq!(ours[1].process_id, second_id);
    assert_eq!(ours[2].process_id, third_id);
    assert!(ours[0].inward_reg_no < ours[1].inward_reg_no);
    assert!(ours[1].inward_reg_no < ours[2].inward_reg_no);
}
