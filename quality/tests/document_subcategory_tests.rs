//! Coverage for document subcategories: category-scoped, optional, UI-only tags on a
//! document — creation/rename, scoping rules (same code allowed under different
//! categories, rejected under the same one), upload-time resolution, and permission
//! gating on the management endpoints.

use quality::{
    quick_create_document_category, quick_create_document_subcategory, quick_create_user,
    quick_onboard_user, quick_setup_admin, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn assert_forbidden(resp: Result<ureq::Response, ureq::Error>, context: &str) {
    assert!(resp.is_err(), "expected failure for {}", context);
    assert_eq!(
        resp.unwrap_err().into_response().unwrap().status(),
        403,
        "expected 403 for {}",
        context
    );
}

fn assert_error(resp: Result<ureq::Response, ureq::Error>, context: &str) {
    assert!(resp.is_err(), "expected failure for {}", context);
}

/// Seeds a brand-new designation holding exactly one permission.
async fn seed_scoped_designation(env: &TestEnv, code: &str, permission_code: &str) {
    sqlx::query("INSERT INTO designations (code, label) VALUES ($1, $1) ON CONFLICT DO NOTHING")
        .bind(code)
        .execute(&env.db_pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO designation_permissions (designation_id, permission_id) \
         SELECT d.designation_id, p.permission_id FROM designations d, permissions p \
         WHERE d.code = $1 AND p.code = $2 \
         ON CONFLICT DO NOTHING",
    )
    .bind(code)
    .bind(permission_code)
    .execute(&env.db_pool)
    .await
    .unwrap();
}

fn list_categories(url: &str, auth_header: &str) -> Vec<serde_json::Value> {
    ureq::post(&format!("{}/api/v1/admin/roles/document-categories", url))
        .set("Authorization", auth_header)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap()
}

/// A newly created subcategory shows up nested under its parent category in the
/// categories listing — there is no separate subcategory-list endpoint.
#[tokio::test]
async fn test_subcategory_is_nested_under_its_category() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "MAINTENANCE", "Maintenance");
    quick_create_document_subcategory(&env, &admin_jwt, "MAINTENANCE", "PLUMBING", "Plumbing");

    let categories = list_categories(&url, &admin_auth);
    let cat = categories
        .iter()
        .find(|c| c["code"] == "MAINTENANCE")
        .expect("category not found");
    let subcats = cat["subcategories"].as_array().unwrap();
    assert_eq!(subcats.len(), 1);
    assert_eq!(subcats[0]["code"], "PLUMBING");
    assert_eq!(subcats[0]["label"], "Plumbing");
    assert_eq!(subcats[0]["category"], "MAINTENANCE");
}

/// Duplicate (category, code) pairs are rejected.
#[tokio::test]
async fn test_duplicate_subcategory_code_under_same_category_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "AMENITIES", "Amenities");
    quick_create_document_subcategory(&env, &admin_jwt, "AMENITIES", "GYM", "Gym");

    let dup = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"category": "AMENITIES", "code": "GYM", "label": "Gym Again"}));
    assert_error(dup, "duplicate subcategory code under the same category");
}

/// The same code is allowed under two different categories — uniqueness is scoped to
/// (category, code), not code alone.
#[tokio::test]
async fn test_same_subcategory_code_allowed_across_different_categories() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "FIN_A", "Finance A");
    quick_create_document_category(&env, &admin_jwt, "FIN_B", "Finance B");
    quick_create_document_subcategory(&env, &admin_jwt, "FIN_A", "AUDIT", "Audit");

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"category": "FIN_B", "code": "AUDIT", "label": "Audit"}));
    assert!(
        resp.is_ok(),
        "same code under a different category should succeed"
    );

    let categories = list_categories(&url, &admin_auth);
    for code in ["FIN_A", "FIN_B"] {
        let cat = categories.iter().find(|c| c["code"] == code).unwrap();
        let subcats = cat["subcategories"].as_array().unwrap();
        assert!(subcats.iter().any(|s| s["code"] == "AUDIT"));
    }
}

/// Creating a subcategory under a category code that doesn't exist is rejected, not a
/// raw FK-violation 500.
#[tokio::test]
async fn test_create_subcategory_under_unknown_category_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"category": "DOES_NOT_EXIST", "code": "X", "label": "X"}));
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("expected 400, got: {:?}", other),
    }
}

/// Renaming changes the label only; the code and parent category are untouched.
#[tokio::test]
async fn test_rename_subcategory_updates_label_not_code() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "SECURITY", "Security");
    quick_create_document_subcategory(&env, &admin_jwt, "SECURITY", "CCTV", "CCTV Footage");

    let categories = list_categories(&url, &admin_auth);
    let subcategory_id = categories.iter().find(|c| c["code"] == "SECURITY").unwrap()
        ["subcategories"][0]["subcategory_id"]
        .as_i64()
        .unwrap();

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/rename",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"subcategory_id": subcategory_id, "label": "Camera Footage"}));
    assert!(resp.is_ok(), "rename should succeed");

    let categories = list_categories(&url, &admin_auth);
    let subcat = &categories.iter().find(|c| c["code"] == "SECURITY").unwrap()["subcategories"][0];
    assert_eq!(subcat["code"], "CCTV");
    assert_eq!(subcat["label"], "Camera Footage");
}

/// Renaming a subcategory ID that doesn't exist is rejected.
#[tokio::test]
async fn test_rename_unknown_subcategory_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/rename",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"subcategory_id": 999999, "label": "Nope"}));
    assert_error(resp, "renaming an unknown subcategory ID");
}

/// Uploading a document with a valid subcategory_code populates the returned metadata
/// with the resolved subcategory's code and label.
#[tokio::test]
async fn test_upload_with_valid_subcategory_populates_metadata() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "NOTICES2", "Notices");
    quick_create_document_subcategory(&env, &admin_jwt, "NOTICES2", "URGENT", "Urgent");

    let url_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "title": "Water Shutdown Notice",
                "filename": "notice.txt",
                "category": "NOTICES2",
                "metadata": {
                    "subcategory_code": "URGENT",
                    "tags": [],
                    "document_date": "2026-07-19"
                }
            }))
            .unwrap()
            .into_json()
            .unwrap();

    let document_id = url_resp["document_id"].as_str().unwrap().to_string();
    let upload_url = url_resp["upload_url"].as_str().unwrap();

    ureq::put(upload_url)
        .set("Content-Type", "text/plain")
        .send_bytes(b"notice body")
        .unwrap();

    ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"document_id": document_id}))
        .unwrap();

    let doc: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/{}/get",
        url, document_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .unwrap()
    .into_json()
    .unwrap();

    assert_eq!(doc["metadata"]["subcategory_code"], "URGENT");
    assert_eq!(doc["metadata"]["subcategory_label"], "Urgent");
}

/// An unknown subcategory_code is rejected at upload time, not silently dropped.
#[tokio::test]
async fn test_upload_with_unknown_subcategory_code_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "NOTICES3", "Notices");

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "title": "Doc",
            "filename": "doc.txt",
            "category": "NOTICES3",
            "metadata": {
                "subcategory_code": "DOES_NOT_EXIST",
                "tags": [],
                "document_date": "2026-07-19"
            }
        }));
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("expected 400, got: {:?}", other),
    }
}

/// A subcategory_code that exists, but under a *different* category than the document
/// being uploaded, is rejected — scoping is enforced, not just code existence.
#[tokio::test]
async fn test_upload_with_subcategory_from_wrong_category_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "CAT_ONE", "Category One");
    quick_create_document_category(&env, &admin_jwt, "CAT_TWO", "Category Two");
    quick_create_document_subcategory(&env, &admin_jwt, "CAT_ONE", "ONLY_HERE", "Only Here");

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "title": "Doc",
            "filename": "doc.txt",
            "category": "CAT_TWO",
            "metadata": {
                "subcategory_code": "ONLY_HERE",
                "tags": [],
                "document_date": "2026-07-19"
            }
        }));
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("expected 400, got: {:?}", other),
    }
}

/// Subcategories are entirely optional — uploading without a subcategory_code (or
/// without metadata at all) still succeeds, with subcategory fields left null.
#[tokio::test]
async fn test_upload_without_subcategory_succeeds() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "NOTICES4", "Notices");

    let url_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "title": "Plain Notice",
                "filename": "notice.txt",
                "category": "NOTICES4",
                "metadata": {
                    "tags": [],
                    "document_date": "2026-07-19"
                }
            }))
            .unwrap()
            .into_json()
            .unwrap();

    let document_id = url_resp["document_id"].as_str().unwrap().to_string();
    let upload_url = url_resp["upload_url"].as_str().unwrap();
    ureq::put(upload_url)
        .set("Content-Type", "text/plain")
        .send_bytes(b"body")
        .unwrap();
    ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"document_id": document_id}))
        .unwrap();

    let doc: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/{}/get",
        url, document_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .unwrap()
    .into_json()
    .unwrap();

    assert!(doc["metadata"]["subcategory_code"].is_null());
    assert!(doc["metadata"]["subcategory_label"].is_null());
}

/// Creating/renaming a subcategory requires `documents.categories.manage` — a designation
/// without it gets 403, same gate as document categories themselves.
#[tokio::test]
async fn test_subcategory_management_requires_categories_manage_permission() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "GATED", "Gated");
    seed_scoped_designation(&env, "unrelated_tester", "users.view").await;

    let rid = quick_create_user(&env, &admin_jwt, "subcat_res@test.com", "A", "113");
    let res_jwt = quick_onboard_user(&env, "subcat_res@test.com", "password123");
    quality::quick_assign_designation(&env, &admin_jwt, rid, "unrelated_tester");
    let res_auth = auth(&res_jwt);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/create",
        url
    ))
    .set("Authorization", &res_auth)
    .send_json(serde_json::json!({"category": "GATED", "code": "X", "label": "X"}));
    assert_forbidden(
        resp,
        "creating a subcategory without documents.categories.manage",
    );
}
