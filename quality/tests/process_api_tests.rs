use quality::{quick_create_user, quick_onboard_user, quick_setup_admin, setup_api_test_env};
use uuid::Uuid;

/// Upload a document via the presigned URL flow:
/// 1. POST /process/upload-url → get {document_id, upload_url}
/// 2. PUT file bytes to the presigned S3 URL
/// 3. POST /process/confirm-upload → confirm the upload
fn upload_doc(url: &str, auth: &str, process_id: &str, filename: &str) -> String {
    // Step 1: Get presigned PUT URL
    let url_resp = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "title": filename,
            "filename": filename,
            "content_type": "application/pdf"
        }))
        .expect("upload-url request failed");

    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    // Step 2: PUT file directly to S3
    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"Simulated PDF content bytes for testing")
        .expect("S3 PUT upload failed");

    // Step 3: Confirm upload
    let confirm_resp = ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "document_id": document_id
        }))
        .expect("confirm-upload request failed");

    let confirm_val: serde_json::Value = confirm_resp.into_json().unwrap();
    assert_eq!(confirm_val["status"].as_str().unwrap(), "confirmed");

    document_id
}

#[tokio::test]
async fn test_process_api_workflow_lifecycle() {
    let env = setup_api_test_env(3108).await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    // Create resident user
    let _rid = quick_create_user(&env, &admin_jwt, "resident_proc@vistas.com", "A", "101");
    let resident_jwt = quick_onboard_user(&env, "resident_proc@vistas.com", "resident_pass_123");
    let resident_auth = format!("Bearer {}", resident_jwt);

    // 1. Fetch process state (should auto-initialize)
    let state_resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "template_id": 1
        }))
        .expect("Failed to get process state");

    assert_eq!(state_resp.status(), 200);
    let state_val: serde_json::Value = state_resp.into_json().unwrap();

    let process_id_str = state_val["process_id"]
        .as_str()
        .expect("process_id should be string");
    let process_id = Uuid::parse_str(process_id_str).expect("process_id should be valid UUID");

    assert_eq!(state_val["template_version_id"].as_i64().unwrap(), 1);
    assert!(!state_val["is_completed"].as_bool().unwrap());

    // Verify steps exist
    let steps = state_val["steps"]
        .as_array()
        .expect("steps should be array");
    assert_eq!(steps.len(), 5);

    // 2. Try to save step 3 fields (without document upload, should fail because Sale Agreement is required)
    let save_err = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": 3,
            "fields": [
                {
                    "template_field_id": 101,
                    "text_value": null,
                    "document_id": null
                }
            ]
        }));
    assert!(save_err.is_err());

    // 3. Upload documents using the presigned URL flow (resident has flat ownership)
    let doc1_id_str = upload_doc(&url, &resident_auth, process_id_str, "sale_agreement.pdf");
    let _doc1_id = Uuid::parse_str(&doc1_id_str).unwrap();

    let doc2_id_str = upload_doc(
        &url,
        &resident_auth,
        process_id_str,
        "possession_letter.pdf",
    );

    // 4. Save step 3 successfully by passing the uploaded documents
    let fields_to_submit = vec![
        (101, doc1_id_str.to_string()),
        (102, doc2_id_str),
        (
            103,
            upload_doc(&url, &resident_auth, process_id_str, "Parking.pdf"),
        ),
        (
            105,
            upload_doc(&url, &resident_auth, process_id_str, "KYC.pdf"),
        ),
        (
            106,
            upload_doc(&url, &resident_auth, process_id_str, "ADDA.pdf"),
        ),
    ];

    let mut fields_payload = serde_json::json!([]);
    for (fid, doc_id) in fields_to_submit {
        fields_payload
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "template_field_id": fid,
                "text_value": null,
                "document_id": doc_id
            }));
    }

    let save_success = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": 3,
            "fields": fields_payload
        }))
        .expect("Failed to save step 3");

    assert_eq!(save_success.status(), 200);
    let save_val: serde_json::Value = save_success.into_json().unwrap();
    assert_eq!(save_val["step"]["template_step_id"].as_i64().unwrap(), 3);
    assert!(save_val["step"]["is_completed"].as_bool().unwrap());

    // 5. Admin can retrieve process state and submit review
    let admin_state_resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": process_id
        }))
        .expect("Admin failed to get process state");
    assert_eq!(admin_state_resp.status(), 200);

    // Complete the other substeps (4, 5, 6) of parent step 2 so it is marked PENDING_REVIEW
    // Step 4: Tenant KYC
    let doc_tenant_kyc = upload_doc(&url, &resident_auth, process_id_str, "TenantKYC.pdf");
    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": 4,
            "fields": [
                {
                    "template_field_id": 107,
                    "text_value": null,
                    "document_id": doc_tenant_kyc
                }
            ]
        }))
        .unwrap();

    // Step 5: Undertaking
    let doc_undertaking = upload_doc(&url, &resident_auth, process_id_str, "Undertaking.pdf");
    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": 5,
            "fields": [
                {
                    "template_field_id": 108,
                    "text_value": null,
                    "document_id": doc_undertaking
                }
            ]
        }))
        .unwrap();

    // Step 6: Payments
    let doc_noc = upload_doc(&url, &resident_auth, process_id_str, "NOC.pdf");
    let doc_shifting = upload_doc(&url, &resident_auth, process_id_str, "Shifting.pdf");
    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": 6,
            "fields": [
                {
                    "template_field_id": 109,
                    "text_value": null,
                    "document_id": doc_noc
                },
                {
                    "template_field_id": 110,
                    "text_value": null,
                    "document_id": doc_shifting
                }
            ]
        }))
        .unwrap();

    // Verify step 2 status is PENDING_REVIEW
    let state_resp_after = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id
        }))
        .unwrap();
    let state_val_after: serde_json::Value = state_resp_after.into_json().unwrap();
    let step2 = state_val_after["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["template_step_id"].as_i64().unwrap() == 2)
        .unwrap();
    assert_eq!(step2["status"].as_str().unwrap(), "PENDING_REVIEW");

    // Admin reviews step 2 and approves it
    let review_resp = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": 2,
            "is_approved": true,
            "notes": "Everything looks perfect!"
        }))
        .expect("Admin review failed");

    assert_eq!(review_resp.status(), 200);
    let review_val: serde_json::Value = review_resp.into_json().unwrap();
    assert_eq!(review_val["status"].as_str().unwrap(), "success");

    // Check review history
    let state_resp_final = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "process_id": process_id
        }))
        .unwrap();
    let state_val_final: serde_json::Value = state_resp_final.into_json().unwrap();
    let reviews_list = state_val_final["reviews"].as_array().unwrap();
    assert!(!reviews_list.is_empty());
    assert!(reviews_list[0]["is_approved"].as_bool().unwrap());
    assert_eq!(
        reviews_list[0]["notes"].as_str().unwrap(),
        "Everything looks perfect!"
    );
}
