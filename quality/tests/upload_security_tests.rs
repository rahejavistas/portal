use quality::{quick_create_user, quick_onboard_user, quick_setup_admin, setup_api_test_env};
use uuid::Uuid;

// ============================================================
// PRESIGNED UPLOAD SECURITY TESTS
// ============================================================

/// Tests that unauthenticated requests to upload-url are rejected.
#[tokio::test]
async fn test_upload_url_requires_authentication() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let err =
        ureq::post(&format!("{}/api/v1/documents/upload-url", url)).send_json(serde_json::json!({
            "process_id": Uuid::new_v4(),
            "title": "test.pdf",
            "filename": "test.pdf"
        }));
    assert!(err.is_err());
    assert_eq!(err.unwrap_err().into_response().unwrap().status(), 401);
}

/// Tests that unauthenticated requests to confirm-upload are rejected.
#[tokio::test]
async fn test_confirm_upload_requires_authentication() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let err = ureq::post(&format!("{}/api/v1/documents/confirm-upload", url)).send_json(
        serde_json::json!({
            "process_id": Uuid::new_v4(),
            "document_id": Uuid::new_v4()
        }),
    );
    assert!(err.is_err());
    assert_eq!(err.unwrap_err().into_response().unwrap().status(), 401);
}

/// Tests that invalid file extensions are rejected at the upload-url stage.
#[tokio::test]
async fn test_upload_url_rejects_invalid_extension() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let _u = quick_create_user(&env, &admin_jwt, "ext_test@test.com", "A", "101");
    let u_jwt = quick_onboard_user(&env, "ext_test@test.com", "password1");
    let u_auth = format!("Bearer {}", u_jwt);

    // Create a process
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let pid = val["process_id"].as_str().unwrap().to_string();

    // Try uploading an .exe file — should be rejected
    let err = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "malware.exe",
            "filename": "malware.exe"
        }));
    assert!(err.is_err());
    assert_eq!(err.unwrap_err().into_response().unwrap().status(), 400);

    // Also try .html (XSS vector)
    let err2 = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "page.html",
            "filename": "page.html"
        }));
    assert!(err2.is_err());
    assert_eq!(err2.unwrap_err().into_response().unwrap().status(), 400);
}

/// Tests that save-step rejects an unconfirmed document.
#[tokio::test]
async fn test_save_step_rejects_unconfirmed_document() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let _u = quick_create_user(&env, &admin_jwt, "unconf@test.com", "A", "101");
    let u_jwt = quick_onboard_user(&env, "unconf@test.com", "password1");
    let u_auth = format!("Bearer {}", u_jwt);

    // Create a process
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let pid = val["process_id"].as_str().unwrap().to_string();

    // Step 1: Get presigned URL (creates unconfirmed document)
    let url_resp = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "unconfirmed.pdf",
            "filename": "unconfirmed.pdf"
        }))
        .expect("upload-url request failed");

    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();

    // Step 2: DO NOT upload to S3, DO NOT confirm
    // Step 3: Try to use the unconfirmed document in save-step — should fail
    let err = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "document_id": document_id}]
        }));
    assert!(err.is_err());
    let err_resp = err.unwrap_err().into_response().unwrap();
    assert_eq!(err_resp.status(), 400);
}

/// Tests that confirm-upload fails if the file was never actually uploaded to S3.
#[tokio::test]
async fn test_confirm_upload_fails_without_s3_file() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let _u = quick_create_user(&env, &admin_jwt, "nofile@test.com", "A", "101");
    let u_jwt = quick_onboard_user(&env, "nofile@test.com", "password1");
    let u_auth = format!("Bearer {}", u_jwt);

    // Create a process
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let pid = val["process_id"].as_str().unwrap().to_string();

    // Get presigned URL
    let url_resp = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "ghost.pdf",
            "filename": "ghost.pdf"
        }))
        .unwrap();

    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();

    // Skip the S3 PUT step entirely, go straight to confirm — should fail
    let err = ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "document_id": document_id
        }));
    assert!(err.is_err());
    let err_resp = err.unwrap_err().into_response().unwrap();
    assert_eq!(err_resp.status(), 400);
}

/// Tests that title sanitization works (empty and overly long titles rejected).
#[tokio::test]
async fn test_upload_url_rejects_bad_titles() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let _u = quick_create_user(&env, &admin_jwt, "title_test@test.com", "A", "101");
    let u_jwt = quick_onboard_user(&env, "title_test@test.com", "password1");
    let u_auth = format!("Bearer {}", u_jwt);

    // Create a process
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let pid = val["process_id"].as_str().unwrap().to_string();

    // Empty title (just whitespace)
    let err = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "   ",
            "filename": "test.pdf"
        }));
    assert!(err.is_err());
    assert_eq!(err.unwrap_err().into_response().unwrap().status(), 400);

    // Title over 200 characters
    let long_title = "A".repeat(201);
    let err2 = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &u_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": long_title,
            "filename": "test.pdf"
        }));
    assert!(err2.is_err());
    assert_eq!(err2.unwrap_err().into_response().unwrap().status(), 400);
}
