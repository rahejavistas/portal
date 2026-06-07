use authentication::{PermissionsManager, UserRole};
use base::flat::FlatIdentifier;
use quality::setup_db_test_env;
use std::sync::Arc;

/// Tests programmatic user creation, credentials seeding, and matching database entries.
#[tokio::test]
async fn test_permissions_manager_user_crud() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let email_exists = pm.email_exists("seeded_res@vistas.com").await.unwrap();
    assert!(!email_exists);

    let email = "seeded_res@vistas.com";
    let wing = "A";
    let flat_num = "101";

    let user = pm
        .create_user_with_credentials(email, Some("resident_password_xyz1"), wing, flat_num)
        .await
        .expect("Failed to create user programmatically");

    assert_eq!(user.email, email);
    assert_eq!(user.wing, wing);
    assert_eq!(user.flat_number, flat_num);
    assert!(user.roles.is_empty());

    let exists_now = pm.email_exists(email).await.unwrap();
    assert!(exists_now);

    let db_user_id: i32 = sqlx::query_scalar("SELECT user_id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(&pool)
        .await
        .expect("Failed to fetch user ID");
    assert_eq!(db_user_id, user.user_id);

    let db_flat_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM flats WHERE wing = $1 AND flat_number = $2)",
    )
    .bind(wing)
    .bind(flat_num)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(db_flat_exists);

    let db_ownership_active: bool = sqlx::query_scalar(
        "SELECT is_active FROM flat_ownerships WHERE user_id = $1 AND is_active = TRUE",
    )
    .bind(user.user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(db_ownership_active);
}

/// Tests that the database constraint allows at most one active owner per flat.
#[tokio::test]
async fn test_active_flat_ownership_constraint() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let wing = "A";
    let flat_num = "0101";

    let _u1 = pm
        .create_user_with_credentials("owner1@vistas.com", Some("password11"), wing, flat_num)
        .await
        .unwrap();

    let attempt = pm
        .create_user_with_credentials("owner2@vistas.com", Some("password22"), wing, flat_num)
        .await;

    assert!(attempt.is_err());
    let err_msg = attempt.unwrap_err();
    assert!(
        err_msg.contains("duplicate key value")
            || err_msg.contains("unique constraint")
            || err_msg.contains("Flat is already actively owned by another user")
    );
}

/// Tests role assignments, active role state transitions, and demotions.
#[tokio::test]
async fn test_permissions_manager_role_transitions() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let user = pm
        .create_user_with_credentials("role_user@vistas.com", Some("password123"), "A", "101")
        .await
        .unwrap();

    pm.set_user_target_role(user.user_id, Some(UserRole::Admin))
        .await
        .unwrap();

    let promoted = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(promoted.roles.contains(&UserRole::Admin));

    pm.set_user_target_role(user.user_id, None).await.unwrap();

    let demoted = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(demoted.roles.is_empty());
}

/// Tests direct document creation visibilities and access resolution queries on DocumentManager.
#[tokio::test]
async fn test_document_manager_visibility_resolution() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let demoted_user = pm
        .create_user_with_credentials("doc_user@vistas.com", Some("password123"), "A", "0101")
        .await
        .unwrap();

    let s3_accessor = base::s3::S3Accessor::new(&base::config::S3Config {
        region: "us-east-1".to_string(),
        bucket_name: "vistas-files".to_string(),
        endpoint_url: Some(format!("http://localhost:{}", quality::TEST_MINIO_PORT)),
        public_endpoint_url: Some(format!("http://localhost:{}", quality::TEST_MINIO_PORT)),
        presigned_expiry: None,
    })
    .await;

    let dm = file_server::document::DocumentManager::new(pool.clone(), s3_accessor);

    let flat_id = FlatIdentifier::new("A", "0101").unwrap();

    let doc1 = dm
        .admin_create_document_atomic(
            "Direct Crate Manual",
            Some("Test document"),
            file_server::document::DocumentCategory::Generic,
            file_server::document::DocumentType::GenericRulebook,
            file_server::document::DocumentAccessLevel::Public,
            None,
            None,
        )
        .await
        .expect("Failed to create Doc 1");

    let doc2 = dm
        .admin_create_document_atomic(
            "Direct A-0101 deed",
            Some("Registry mapping"),
            file_server::document::DocumentCategory::ResidentSpecific,
            file_server::document::DocumentType::ResidentProperty,
            file_server::document::DocumentAccessLevel::Private,
            Some(&flat_id),
            None,
        )
        .await
        .expect("Failed to create Doc 2");

    let allowed_ids = dm.get_allowed_document_ids(&demoted_user).await.unwrap();
    assert!(allowed_ids.contains(&doc1.document.document_id));
    assert!(allowed_ids.contains(&doc2.document.document_id));

    let flat_b_id = FlatIdentifier::new("B", "0202").unwrap();
    sqlx::query(
        "INSERT INTO flats (wing, flat_number) VALUES ('B', '0202') ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .unwrap();

    let doc3 = dm
        .admin_create_document_atomic(
            "Direct B-0202 deed",
            Some("Unrelated flat registry"),
            file_server::document::DocumentCategory::ResidentSpecific,
            file_server::document::DocumentType::ResidentProperty,
            file_server::document::DocumentAccessLevel::Private,
            Some(&flat_b_id),
            None,
        )
        .await
        .unwrap();

    let allowed_ids_after = dm.get_allowed_document_ids(&demoted_user).await.unwrap();
    assert!(!allowed_ids_after.contains(&doc3.document.document_id));

    let visible_list = dm.list_visible_documents(&demoted_user).await.unwrap();
    let visible_list_ids: Vec<uuid::Uuid> = visible_list
        .iter()
        .map(|d| d.document.document_id)
        .collect();

    assert!(visible_list_ids.contains(&doc1.document.document_id));
    assert!(visible_list_ids.contains(&doc2.document.document_id));
    assert!(!visible_list_ids.contains(&doc3.document.document_id));
}
