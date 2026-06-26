use quality::{quick_setup_admin, setup_api_test_env};

/// Tests the document upload workflow via multipart/form-data.
#[tokio::test]
async fn test_multipart_document_upload() {
    let env = setup_api_test_env(3106).await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    let payload = serde_json::json!({
        "title": "S3 Upload Integration Test",
        "category": "GENERIC",
        "document_type": "GENERIC_RULEBOOK",
        "filename": "hello.txt"
    });

    // Step 1: Get presigned URL
    let url_resp = ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
        .set("Authorization", &admin_auth)
        .send_json(payload)
        .expect("Failed to create upload url");

    assert_eq!(url_resp.status(), 201);
    let val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = val["document_id"].as_str().unwrap().to_string();
    let presigned_url = val["upload_url"].as_str().unwrap().to_string();

    // Step 2: PUT file directly to S3
    let mock_bytes = b"Hello world, this is a test document uploaded via presigned URL!";
    let put_resp = ureq::put(&presigned_url)
        .set("Content-Type", "text/plain")
        .send_bytes(mock_bytes)
        .expect("Failed to PUT file to S3");

    let status = put_resp.status();
    let _body = put_resp.into_string().unwrap();
    assert_eq!(status, 200);

    let confirm_resp = ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "document_id": document_id
        }));

    if let Err(ureq::Error::Status(code, response)) = confirm_resp {
        panic!(
            "Failed to confirm upload: {} - {}",
            code,
            response.into_string().unwrap()
        );
    }

    // Verify document details via GET
    let get_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/{}/get",
        url, document_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("Failed to get document");

    let get_val: serde_json::Value = get_resp.into_json().unwrap();
    assert_eq!(
        get_val["document"]["title"].as_str().unwrap(),
        "S3 Upload Integration Test"
    );
    assert!(get_val["download_url"]
        .as_str()
        .unwrap()
        .starts_with("http"));
}
