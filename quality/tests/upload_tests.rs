use quality::{quick_setup_admin, setup_api_test_env};

/// Tests the document upload workflow via multipart/form-data.
#[tokio::test]
async fn test_multipart_document_upload() {
    let env = setup_api_test_env(3106).await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    // 1. Construct multipart payload
    let boundary = "----WebKitFormBoundaryUploadTest123";
    let payload = serde_json::json!({
        "title": "S3 Upload Integration Test",
        "category": "GENERIC",
        "document_type": "GENERIC_RULEBOOK"
    })
    .to_string();

    let mut body = Vec::new();

    // Payload part
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"payload\"\r\n\r\n");
    body.extend_from_slice(payload.as_bytes());
    body.extend_from_slice(b"\r\n");

    // File part
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"hello.txt\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    body.extend_from_slice(b"Hello world, this is a test document uploaded via multipart!");
    body.extend_from_slice(b"\r\n");

    // End boundary
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    // 2. Send request
    let resp = ureq::post(&format!("{}/api/v1/admin/documents/create", url))
        .set("Authorization", &admin_auth)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .expect("Failed to execute multipart upload");

    assert_eq!(resp.status(), 201);

    let resp_val: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(
        resp_val["document"]["title"].as_str().unwrap(),
        "S3 Upload Integration Test"
    );
    assert!(resp_val["download_url"]
        .as_str()
        .unwrap()
        .starts_with("http"));
}
