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
        "SELECT fo.is_active FROM flat_ownerships fo \
         JOIN flat_ownership_users fou ON fou.ownership_id = fo.ownership_id AND fou.is_active = TRUE \
         WHERE fou.user_id = $1 AND fo.is_active = TRUE",
    )
    .bind(user.user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(db_ownership_active);
}

/// A flat takes two owners with equal standing and rejects a third.
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

    pm.create_user_with_credentials(
        "owner2@vistas.com",
        Some("password22"),
        wing,
        flat_num,
        None,
    )
    .await
    .expect("a second owner is allowed");

    let attempt = pm
        .create_user_with_credentials(
            "owner3@vistas.com",
            Some("password33"),
            wing,
            flat_num,
            None,
        )
        .await;

    assert!(matches!(
        attempt.expect_err("a third owner must be rejected"),
        authentication::CreateUserError::FlatFull
    ));
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

    let allowed_ids = dm.flat_view_document_ids(&demoted_user).await.unwrap();
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

    let allowed_ids_after = dm.flat_view_document_ids(&demoted_user).await.unwrap();
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

/// A document manager or category reader opening their own flat's documents view sees
/// only that flat's documents and public ones — never another flat's — while the admin
/// list still returns everything.
#[tokio::test]
async fn test_flat_view_excludes_other_flats_even_with_document_access() {
    let env = setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let pm = Arc::new(PermissionsManager::new(pool.clone()));

    let resident = pm
        .create_user_with_credentials(
            "doc_manager@vistas.com",
            Some("password123"),
            "A",
            "0101",
            None,
        )
        .await
        .unwrap();
    pm.create_designation("doc_managers", "Doc Managers", None)
        .await
        .unwrap();
    pm.toggle_designation_permission("doc_managers", "documents.all.manage", true, None)
        .await
        .unwrap();
    pm.create_designation("category_readers", "Category Readers", None)
        .await
        .unwrap();
    pm.assign_designation(resident.user_id, "doc_managers", None)
        .await
        .unwrap();
    pm.assign_designation(resident.user_id, "category_readers", None)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO document_category_designations (category, designation_id, can_read, can_manage) \
         SELECT 'RESIDENT_SPECIFIC', designation_id, TRUE, FALSE FROM designations WHERE code = 'category_readers'",
    )
    .execute(&pool)
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

    ensure_wing_exists_for_test(&pool, "B").await;
    sqlx::query(
        "INSERT INTO flats (wing, flat_number) VALUES ('B', '0202') ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .unwrap();

    let create = |title: &'static str, category: &'static str, flat: Option<FlatIdentifier>| {
        let dm = &dm;
        async move {
            dm.admin_create_upload_intent(
                file_server::document::AdminUploadIntentInput {
                    title,
                    description: None,
                    category,
                    metadata: None,
                    content_type: "application/pdf",
                    uploaded_by: None,
                },
                flat.as_ref(),
            )
            .await
            .unwrap()
            .0
            .document
            .document_id
        }
    };
    let public_doc = create("Notice", "PUBLIC", None).await;
    let own_flat_doc = create(
        "A-0101 deed",
        "RESIDENT_SPECIFIC",
        Some(FlatIdentifier::new("A", "0101").unwrap()),
    )
    .await;
    let other_flat_doc = create(
        "B-0202 deed",
        "RESIDENT_SPECIFIC",
        Some(FlatIdentifier::new("B", "0202").unwrap()),
    )
    .await;

    let visible: Vec<uuid::Uuid> = dm
        .list_visible_documents(&pm.get_user_by_id(resident.user_id).await.unwrap().unwrap())
        .await
        .unwrap()
        .iter()
        .map(|d| d.document.document_id)
        .collect();
    assert!(visible.contains(&public_doc));
    assert!(visible.contains(&own_flat_doc));
    assert!(
        !visible.contains(&other_flat_doc),
        "another flat's document must never appear in this user's flat view"
    );

    let admin_view: Vec<uuid::Uuid> = dm
        .admin_list_all_documents()
        .await
        .unwrap()
        .iter()
        .map(|d| d.document.document_id)
        .collect();
    assert!(admin_view.contains(&other_flat_doc));
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
        .create_upload_intent(&resident, "Resident Presigned Doc", "application/pdf", None)
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
