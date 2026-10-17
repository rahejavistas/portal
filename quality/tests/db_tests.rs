use authentication::PermissionsManager;
use base::flat::FlatIdentifier;
use quality::{ensure_wing_exists_for_test, setup_db_test_env};
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
        .create_user_with_credentials(email, Some("resident_password_xyz1"), wing, flat_num, None)
        .await
        .expect("Failed to create user programmatically");

    assert_eq!(user.email, email);
    assert_eq!(user.wing, wing);
    assert_eq!(user.flat_number, flat_num);
    assert!(user.designations.is_empty());

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
        .create_user_with_credentials(
            "owner1@vistas.com",
            Some("password11"),
            wing,
            flat_num,
            None,
        )
        .await
        .unwrap();

    let attempt = pm
        .create_user_with_credentials(
            "owner2@vistas.com",
            Some("password22"),
            wing,
            flat_num,
            None,
        )
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
        .create_user_with_credentials(
            "role_user@vistas.com",
            Some("password123"),
            "A",
            "101",
            None,
        )
        .await
        .unwrap();

    pm.create_designation("committee", "Committee", None)
        .await
        .unwrap();
    pm.assign_designation(user.user_id, "committee", None)
        .await
        .unwrap();

    let promoted = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(promoted.designations.contains(&"committee".to_string()));

    pm.revoke_designation(user.user_id, "committee", None)
        .await
        .unwrap();

    let demoted = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(demoted.designations.is_empty());
}

/// Tests direct document creation visibilities and access resolution queries on DocumentManager.
#[tokio::test]
async fn test_document_manager_visibility_resolution() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let demoted_user = pm
        .create_user_with_credentials(
            "doc_user@vistas.com",
            Some("password123"),
            "A",
            "0101",
            None,
        )
        .await
        .unwrap();

    std::env::set_var("AWS_ACCESS_KEY_ID", "admin");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "password");

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
        .admin_create_upload_intent(
            file_server::document::AdminUploadIntentInput {
                title: "Direct Crate Manual",
                description: Some("Test document"),
                category: "PUBLIC",
                metadata: None,
                content_type: "application/pdf",
                uploaded_by: None,
            },
            None,
        )
        .await
        .expect("Failed to create Doc 1")
        .0;

    let doc2 = dm
        .admin_create_upload_intent(
            file_server::document::AdminUploadIntentInput {
                title: "Direct A-0101 deed",
                description: Some("Registry mapping"),
                category: "RESIDENT_SPECIFIC",
                metadata: None,
                content_type: "application/pdf",
                uploaded_by: None,
            },
            Some(&flat_id),
        )
        .await
        .expect("Failed to create Doc 2")
        .0;

    let allowed_ids = dm.get_allowed_document_ids(&demoted_user).await.unwrap();
    assert!(allowed_ids.contains(&doc1.document.document_id));
    assert!(allowed_ids.contains(&doc2.document.document_id));

    let flat_b_id = FlatIdentifier::new("B", "0202").unwrap();
    ensure_wing_exists_for_test(&pool, "B").await;
    sqlx::query(
        "INSERT INTO flats (wing, flat_number) VALUES ('B', '0202') ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .unwrap();

    let doc3 = dm
        .admin_create_upload_intent(
            file_server::document::AdminUploadIntentInput {
                title: "Direct B-0202 deed",
                description: Some("Unrelated flat registry"),
                category: "RESIDENT_SPECIFIC",
                metadata: None,
                content_type: "application/pdf",
                uploaded_by: None,
            },
            Some(&flat_b_id),
        )
        .await
        .unwrap()
        .0;

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

/// Verifies `documents.uploaded_by` is recorded correctly across every document creation
/// path: admin uploads (with and without an uploader), the resident presigned-upload flow,
/// and the legacy server-proxied resident upload flow.
#[tokio::test]
async fn test_document_uploaded_by_tracking() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let admin = pm
        .create_seeded_user("uploader_admin@vistas.com", "super_admin")
        .await
        .unwrap();
    let resident = pm
        .create_user_with_credentials(
            "uploader_res@vistas.com",
            Some("password123"),
            "C",
            "0303",
            None,
        )
        .await
        .unwrap();

    std::env::set_var("AWS_ACCESS_KEY_ID", "admin");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "password");

    let s3_accessor = base::s3::S3Accessor::new(&base::config::S3Config {
        region: "us-east-1".to_string(),
        bucket_name: "vistas-files".to_string(),
        endpoint_url: Some(format!("http://localhost:{}", quality::TEST_MINIO_PORT)),
        public_endpoint_url: Some(format!("http://localhost:{}", quality::TEST_MINIO_PORT)),
        presigned_expiry: None,
    })
    .await;

    let dm = file_server::document::DocumentManager::new(pool.clone(), s3_accessor);

    // Admin upload with a known uploader records that uploader's user_id.
    let admin_doc = dm
        .admin_create_upload_intent(
            file_server::document::AdminUploadIntentInput {
                title: "Admin Uploaded Doc",
                description: None,
                category: "PUBLIC",
                metadata: None,
                content_type: "application/pdf",
                uploaded_by: Some(admin.user_id),
            },
            None,
        )
        .await
        .unwrap()
        .0;
    assert_eq!(admin_doc.document.uploaded_by, Some(admin.user_id));

    // Admin upload with no uploader supplied leaves the column NULL.
    let system_doc = dm
        .admin_create_upload_intent(
            file_server::document::AdminUploadIntentInput {
                title: "System Doc",
                description: None,
                category: "PUBLIC",
                metadata: None,
                content_type: "application/pdf",
                uploaded_by: None,
            },
            None,
        )
        .await
        .unwrap()
        .0;
    assert_eq!(system_doc.document.uploaded_by, None);

    // Resident presigned-upload flow records the resident's own user_id.
    let (resident_doc, _upload_url, _key) = dm
        .create_upload_intent(&resident, "Resident Presigned Doc", "application/pdf")
        .await
        .unwrap();
    assert_eq!(resident_doc.document.uploaded_by, Some(resident.user_id));

    // Legacy server-proxied resident upload flow also records the uploader.
    let proxied_doc = dm
        .user_create_document_atomic(&resident, "Resident Proxied Doc", None)
        .await
        .unwrap();
    assert_eq!(proxied_doc.document.uploaded_by, Some(resident.user_id));

    // Sanity-check the value is actually persisted in the DB, not just echoed back in-memory.
    let stored_uploaded_by: Option<i32> =
        sqlx::query_scalar("SELECT uploaded_by FROM documents WHERE document_id = $1")
            .bind(resident_doc.document.document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored_uploaded_by, Some(resident.user_id));
}
