use quality::{quick_create_user, quick_onboard_user, quick_setup_admin, setup_api_test_env};
use uuid::Uuid;

pub fn create_process_multipart_body(
    boundary: &str,
    process_id: &str,
    title: &str,
    file_bytes: &[u8],
    filename: &str,
    file_content_type: &str,
) -> Vec<u8> {
    let mut body = Vec::new();

    // process_id
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"process_id\"\r\n\r\n");
    body.extend_from_slice(process_id.as_bytes());
    body.extend_from_slice(b"\r\n");

    // title
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"title\"\r\n\r\n");
    body.extend_from_slice(title.as_bytes());
    body.extend_from_slice(b"\r\n");

    // file
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file_content_type).as_bytes());
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    body
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
            "template_id": "tenant_onboarding_v1"
        }))
        .expect("Failed to get process state");

    assert_eq!(state_resp.status(), 200);
    let state_val: serde_json::Value = state_resp.into_json().unwrap();

    let process_id_str = state_val["process_id"]
        .as_str()
        .expect("process_id should be string");
    let process_id = Uuid::parse_str(process_id_str).expect("process_id should be valid UUID");

    assert_eq!(
        state_val["template_id"].as_str().unwrap(),
        "tenant_onboarding_v1"
    );
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

    // 3. Upload a document specifically for the process (using admin to preserve resident rate-limiting)
    let boundary = "----TestProcessUploadBoundary123";
    let multipart_body = create_process_multipart_body(
        boundary,
        process_id_str,
        "Sale Agreement Copy",
        b"Simulated PDF content for sale agreement document",
        "sale_agreement.pdf",
        "application/pdf",
    );

    let upload_resp = ureq::post(&format!("{}/api/v1/process/upload", url))
        .set("Authorization", &admin_auth)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&multipart_body)
        .expect("Process document upload failed");

    assert_eq!(upload_resp.status(), 201);
    let upload_val: serde_json::Value = upload_resp.into_json().unwrap();
    let doc1_id_str = upload_val["document"]["document_id"]
        .as_str()
        .expect("document_id should be present");
    let _doc1_id = Uuid::parse_str(doc1_id_str).unwrap();

    // Upload possession letter document (field 102) using admin
    let multipart_body2 = create_process_multipart_body(
        boundary,
        process_id_str,
        "Possession Letter Copy",
        b"Possession letter PDF contents",
        "possession_letter.pdf",
        "application/pdf",
    );
    let upload_resp2 = ureq::post(&format!("{}/api/v1/process/upload", url))
        .set("Authorization", &admin_auth)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&multipart_body2)
        .expect("Possession upload failed");
    let doc2_val: serde_json::Value = upload_resp2.into_json().unwrap();
    let doc2_id_str = doc2_val["document"]["document_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 4. Save step 3 successfully by passing the uploaded documents
    let fields_to_submit = vec![
        (101, doc1_id_str.to_string()),
        (102, doc2_id_str),
        (
            103,
            quick_upload_doc(&url, &admin_auth, process_id_str, "Parking.pdf"),
        ),
        (
            105,
            quick_upload_doc(&url, &admin_auth, process_id_str, "KYC.pdf"),
        ),
        (
            106,
            quick_upload_doc(&url, &admin_auth, process_id_str, "ADDA.pdf"),
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
    // Step 4: Tenant KYC (uploaded as admin)
    let doc_tenant_kyc = quick_upload_doc(&url, &admin_auth, process_id_str, "TenantKYC.pdf");
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

    // Step 5: Undertaking (uploaded as admin)
    let doc_undertaking = quick_upload_doc(&url, &admin_auth, process_id_str, "Undertaking.pdf");
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

    // Step 6: Payments (uploaded as admin)
    let doc_noc = quick_upload_doc(&url, &admin_auth, process_id_str, "NOC.pdf");
    let doc_shifting = quick_upload_doc(&url, &admin_auth, process_id_str, "Shifting.pdf");
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

fn quick_upload_doc(url: &str, auth: &str, process_id: &str, filename: &str) -> String {
    let boundary = "----TestProcessUploadBoundary123";
    let body = create_process_multipart_body(
        boundary,
        process_id,
        filename,
        b"Simulated document bytes",
        filename,
        "application/pdf",
    );
    let resp = ureq::post(&format!("{}/api/v1/process/upload", url))
        .set("Authorization", auth)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .expect("Quick upload failed");
    let val: serde_json::Value = resp.into_json().unwrap();
    val["document"]["document_id"].as_str().unwrap().to_string()
}
