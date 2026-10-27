use quality::{
    quick_assign_designation, quick_create_designation, quick_create_user, quick_onboard_user,
    quick_setup_admin, quick_toggle_designation_permission, setup_api_test_env,
};
use uuid::Uuid;

/// Upload a document using the presigned URL flow:
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
        .send_bytes(b"fake pdf content bytes")
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

/// Helper: create a resident, init their process, return (process_id_str, resident_auth, admin_auth)
fn setup_process(url: &str, env: &quality::TestEnv) -> (String, String, String) {
    let admin_jwt = quick_setup_admin(env);
    let admin_auth = format!("Bearer {}", admin_jwt);
    let _rid = quick_create_user(env, &admin_jwt, "res@test.com", "A", "101");
    let res_jwt = quick_onboard_user(env, "res@test.com", "password123");
    let res_auth = format!("Bearer {}", res_jwt);

    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let pid = val["process_id"].as_str().unwrap().to_string();
    (pid, res_auth, admin_auth)
}

// ============================================================
// POST /api/v1/process/state
// ============================================================

#[tokio::test]
async fn test_state_auto_init_returns_valid_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, _) = setup_process(&url, &env);

    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();

    assert_eq!(val["template_version_id"].as_i64().unwrap(), 1);
    assert!(!val["is_completed"].as_bool().unwrap());
    assert_eq!(val["steps"].as_array().unwrap().len(), 5);
    // First step should be OPEN, second should be LOCKED
    let steps = val["steps"].as_array().unwrap();
    assert_eq!(steps[0]["status"].as_str().unwrap(), "OPEN");
    assert_eq!(steps[1]["status"].as_str().unwrap(), "LOCKED");
}

#[tokio::test]
async fn test_state_creates_new_process_each_time() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid1, res_auth, _) = setup_process(&url, &env);

    // Call again with template_id — should create a new process as per recent changes
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    assert_ne!(val["process_id"].as_str().unwrap(), pid1);
}

#[tokio::test]
async fn test_state_admin_can_start_own_process_without_process_id() {
    // A user holding process.reviewer (e.g. super_admin) can still start their own
    // process by template_id — reviewing others' steps doesn't preclude having one's own.
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(serde_json::json!({"template_id": 1}));
    assert!(resp.is_ok());
}

#[tokio::test]
async fn test_state_invalid_process_id_returns_404() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let err = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(serde_json::json!({"process_id": Uuid::new_v4()}));
    assert!(err.is_err());
}

// ============================================================
// POST /api/v1/process/upload
// ============================================================

#[tokio::test]
async fn test_upload_creates_document_for_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, _admin_auth) = setup_process(&url, &env);

    let doc_id = upload_doc(&url, &res_auth, &pid, "test.pdf");
    assert!(Uuid::parse_str(&doc_id).is_ok());
}

#[tokio::test]
async fn test_upload_rejects_empty_file() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, _, admin_auth) = setup_process(&url, &env);

    let boundary = "----B";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"process_id\"\r\n\r\n");
    body.extend_from_slice(pid.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"e.pdf\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/pdf\r\n\r\n");
    // empty file
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let err = ureq::post(&format!("{}/api/v1/process/upload", url))
        .set("Authorization", &admin_auth)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body);
    assert!(err.is_err());
}

// ============================================================
// POST /api/v1/process/save-step
// ============================================================

#[tokio::test]
async fn test_save_step_missing_required_document_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, _) = setup_process(&url, &env);

    // Submit step 3 with null document_id for a required document field
    let err = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "text_value": null, "document_id": null}]
        }));
    assert!(err.is_err());
}

#[tokio::test]
async fn test_save_step_invalid_document_id_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, _) = setup_process(&url, &env);

    let err = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "document_id": Uuid::new_v4()}]
        }));
    assert!(err.is_err());
}

/// A resident must not be able to attach a non-Resident-Specific document (e.g. a PUBLIC
/// document, which is visible to them but not flat-scoped) to a process field, even though
/// it passes the ownership/visibility check in `get_document`.
#[tokio::test]
async fn test_save_step_rejects_non_resident_specific_document() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    // Admin creates and confirms a PUBLIC document via the presigned upload flow.
    let url_resp = ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "title": "Society Notice",
            "category": "PUBLIC",
            "filename": "notice.pdf"
        }))
        .expect("admin upload-url request failed");
    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let public_doc_id = url_val["document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"fake pdf content bytes")
        .expect("S3 PUT upload failed");

    ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "document_id": public_doc_id }))
        .expect("admin confirm-upload request failed");

    // Resident attempts to attach the PUBLIC document to a process document field.
    let err = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "document_id": public_doc_id}]
        }));

    match err {
        Err(ureq::Error::Status(code, response)) => {
            assert_eq!(code, 400);
            let body = response.into_string().unwrap();
            assert!(
                body.contains("Resident Specific"),
                "unexpected error body: {}",
                body
            );
        }
        other => panic!("expected a 400 rejection, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_save_step_locked_step_fails() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, _, admin_auth) = setup_process(&url, &env);

    // Step 7 is LOCKED initially
    let err = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 7,
            "fields": [{"template_field_id": 111, "text_value": "test"}]
        }));
    assert!(err.is_err());
}

#[tokio::test]
async fn test_save_step_completes_substep() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, _admin_auth) = setup_process(&url, &env);

    // Upload docs and save step 3 (all required fields)
    let fields: Vec<serde_json::Value> = [101, 102, 103, 105, 106]
        .iter()
        .map(|fid| {
            let doc = upload_doc(&url, &res_auth, &pid, &format!("f{}.pdf", fid));
            serde_json::json!({"template_field_id": fid, "document_id": doc})
        })
        .collect();

    let resp = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": fields
        }))
        .unwrap();

    let val: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(val["status"].as_str().unwrap(), "success");
    assert!(val["step"]["is_completed"].as_bool().unwrap());
}

// ============================================================
// POST /api/v1/admin/process/review
// ============================================================

#[tokio::test]
async fn test_review_approve_completes_step() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    // Complete all substeps of step 2 to trigger PENDING_REVIEW
    for (step_id, field_ids) in [
        (3, vec![101, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let fields: Vec<serde_json::Value> = field_ids
            .iter()
            .map(|fid| {
                let doc = upload_doc(&url, &res_auth, &pid, &format!("f{}.pdf", fid));
                serde_json::json!({"template_field_id": fid, "document_id": doc})
            })
            .collect();
        ureq::post(&format!("{}/api/v1/process/save-step", url))
            .set("Authorization", &res_auth)
            .send_json(serde_json::json!({"process_id": pid, "template_step_id": step_id, "fields": fields}))
            .unwrap();
    }

    // Verify step 2 is PENDING_REVIEW
    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap()
        .into_json()
        .unwrap();
    let step2 = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["template_step_id"].as_i64().unwrap() == 2)
        .unwrap();
    assert_eq!(step2["status"].as_str().unwrap(), "PENDING_REVIEW");

    // Approve
    let resp = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": pid, "template_step_id": 2,
            "is_approved": true, "notes": "LGTM"
        }))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(val["status"].as_str().unwrap(), "success");
    assert!(val["is_completed"].as_bool().unwrap());

    // Verify step 2 is COMPLETED and step 7 is now OPEN
    let state2: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap()
        .into_json()
        .unwrap();
    let steps = state2["steps"].as_array().unwrap();
    let s2 = steps
        .iter()
        .find(|s| s["template_step_id"].as_i64().unwrap() == 2)
        .unwrap();
    let s7 = steps
        .iter()
        .find(|s| s["template_step_id"].as_i64().unwrap() == 7)
        .unwrap();
    assert_eq!(s2["status"].as_str().unwrap(), "COMPLETED");
    assert_eq!(s7["status"].as_str().unwrap(), "OPEN");

    // Verify review shows in history
    assert!(!state2["reviews"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_review_reject_reopens_step() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    // Complete all substeps of step 2
    for (step_id, field_ids) in [
        (3, vec![101, 102, 103, 105, 106]),
        (4, vec![107]),
        (5, vec![108]),
        (6, vec![109, 110]),
    ] {
        let fields: Vec<serde_json::Value> = field_ids
            .iter()
            .map(|fid| {
                let doc = upload_doc(&url, &res_auth, &pid, &format!("f{}.pdf", fid));
                serde_json::json!({"template_field_id": fid, "document_id": doc})
            })
            .collect();
        ureq::post(&format!("{}/api/v1/process/save-step", url))
            .set("Authorization", &res_auth)
            .send_json(serde_json::json!({"process_id": pid, "template_step_id": step_id, "fields": fields}))
            .unwrap();
    }

    // Reject
    let resp = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": pid, "template_step_id": 2,
            "is_approved": false, "notes": "Redo docs"
        }))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    assert!(!val["is_completed"].as_bool().unwrap());

    // Verify step 2 is back to OPEN
    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap()
        .into_json()
        .unwrap();
    let s2 = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["template_step_id"].as_i64().unwrap() == 2)
        .unwrap();
    assert_eq!(s2["status"].as_str().unwrap(), "OPEN");
}

// ============================================================
// POST /api/v1/process/download-url
// ============================================================

#[tokio::test]
async fn test_download_url_returns_presigned_url() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    let doc_id = upload_doc(&url, &res_auth, &pid, "test.pdf");
    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "text_value": null, "document_id": doc_id}]
        }))
        .expect("save-step should attach the document to the process field");

    let resp = ureq::post(&format!("{}/api/v1/process/download-url", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"process_id": pid, "document_id": doc_id}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let dl_url = val["download_url"].as_str().unwrap();
    assert!(dl_url.starts_with("http"));
    assert!(dl_url.contains(&doc_id));
}

#[tokio::test]
async fn test_download_url_wrong_process_returns_404() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    let doc_id = upload_doc(&url, &res_auth, &pid, "test.pdf");

    // Use a random process_id — doc doesn't belong to it
    let err = ureq::post(&format!("{}/api/v1/process/download-url", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"process_id": Uuid::new_v4(), "document_id": doc_id}));
    assert!(err.is_err());
}

/// A `process.reviewer` holder (not super_admin, not the document's owner, and not
/// holding any document-category permission at all) must still be able to download a
/// document the resident attached to a process they're authorized to review — being
/// cleared to view the process authorizes its linked documents too. Regression test for
/// a bug where `process_download_url` re-applied the document's own per-user visibility
/// rules (category/flat-based) on top of the already-passed process-review check, so a
/// reviewer got 403 on any document not separately visible to them (e.g. a
/// RESIDENT_SPECIFIC upload scoped to a flat the reviewer doesn't own).
#[tokio::test]
async fn test_reviewer_can_download_document_attached_to_reviewable_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    // Resident owns the process and self-uploads a RESIDENT_SPECIFIC document to it,
    // scoped to their own flat (A-101) — a flat the reviewer below does not own.
    quick_create_user(&env, &admin_jwt, "reviewee2@test.com", "A", "101");
    let res_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "reviewee2@test.com", "password123")
    );
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let pid = resp.into_json::<serde_json::Value>().unwrap()["process_id"]
        .as_str()
        .unwrap()
        .to_string();
    let doc_id = upload_doc(&url, &res_auth, &pid, "reviewee_doc.pdf");

    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "text_value": null, "document_id": doc_id}]
        }))
        .expect("save-step should attach the document to the process field");

    // A scoped designation holding exactly `process.reviewer` — nothing else, no
    // document-category grants, no `documents.all.manage`, no shared flat.
    quick_create_designation(&env, &admin_jwt, "doc_reviewer", "Doc Reviewer");
    quick_toggle_designation_permission(&env, &admin_jwt, "doc_reviewer", "process.reviewer", true);
    let reviewer_id = quick_create_user(&env, &admin_jwt, "reviewer2@test.com", "B", "201");
    let reviewer_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "reviewer2@test.com", "password123")
    );
    quick_assign_designation(&env, &admin_jwt, reviewer_id, "doc_reviewer");

    // Sanity: this reviewer genuinely has no other route to this document.
    let direct_get = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &reviewer_auth)
        .send_json(serde_json::json!({}));
    assert!(
        direct_get.is_err(),
        "reviewer should NOT have generic document-list access to this resident-scoped doc"
    );

    // But via the process-review path, it must succeed.
    let via_process = ureq::post(&format!("{}/api/v1/process/download-url", url))
        .set("Authorization", &reviewer_auth)
        .send_json(serde_json::json!({"process_id": pid, "document_id": doc_id}));
    assert!(
        via_process.is_ok(),
        "process.reviewer should be able to download a document attached to a process they can review"
    );
    let val: serde_json::Value = via_process.unwrap().into_json().unwrap();
    assert!(val["download_url"].as_str().unwrap().contains(&doc_id));
}

/// A reviewer must not be able to download a document that was never attached to the process.
#[tokio::test]
async fn test_reviewer_cannot_download_document_not_attached_to_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "reviewee3@test.com", "A", "102");
    let res_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "reviewee3@test.com", "password123")
    );
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let pid = resp.into_json::<serde_json::Value>().unwrap()["process_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Uploaded to the resident's own library, but never attached to any field
    // of this (or any) process via save-step.
    let unattached_doc_id = upload_doc(&url, &res_auth, &pid, "unrelated_private_doc.pdf");

    quick_create_designation(&env, &admin_jwt, "doc_reviewer3", "Doc Reviewer 3");
    quick_toggle_designation_permission(
        &env,
        &admin_jwt,
        "doc_reviewer3",
        "process.reviewer",
        true,
    );
    let reviewer_id = quick_create_user(&env, &admin_jwt, "reviewer3@test.com", "B", "202");
    let reviewer_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "reviewer3@test.com", "password123")
    );
    quick_assign_designation(&env, &admin_jwt, reviewer_id, "doc_reviewer3");

    let via_process = ureq::post(&format!("{}/api/v1/process/download-url", url))
        .set("Authorization", &reviewer_auth)
        .send_json(serde_json::json!({"process_id": pid, "document_id": unattached_doc_id}));
    assert!(
        via_process.is_err(),
        "process.reviewer must not be able to download a document that was never attached to the process, \
         even if they're authorized to review that process"
    );
}

// ============================================================
// Access Control
// ============================================================

#[tokio::test]
async fn test_user_cannot_access_other_users_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    // Create two users
    let _u1 = quick_create_user(&env, &admin_jwt, "u1@test.com", "A", "101");
    let u1_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "u1@test.com", "password1")
    );
    let _u2 = quick_create_user(&env, &admin_jwt, "u2@test.com", "A", "102");
    let u2_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "u2@test.com", "password2")
    );

    // User 1 initializes their process
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &u1_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    let u1_pid = val["process_id"].as_str().unwrap();

    // User 2 tries to access User 1's process — should be forbidden
    let err = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &u2_auth)
        .send_json(serde_json::json!({"process_id": u1_pid}));
    assert!(err.is_err());
}

#[tokio::test]
async fn test_admin_process_upload_creates_downloadable_document() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    // 1. Admin uploads document to the process via the new endpoint
    let url_resp = ureq::post(&format!("{}/api/v1/admin/process/upload-url", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "Admin SLA",
            "filename": "admin_sla.pdf",
            "content_type": "application/pdf"
        }))
        .expect("admin upload-url request failed");

    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"admin pdf content bytes")
        .expect("S3 PUT upload failed");

    let confirm_resp = ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "document_id": document_id
        }))
        .expect("admin confirm-upload request failed");

    let confirm_val: serde_json::Value = confirm_resp.into_json().unwrap();
    assert_eq!(confirm_val["status"].as_str().unwrap(), "confirmed");

    // 2. Test the download as resident - Should succeed because the backend linked the flat!
    let download_resp = ureq::post(&format!("{}/api/v1/process/download-url", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "document_id": document_id
        }))
        .expect("Resident download process document failed");

    let download_val: serde_json::Value = download_resp.into_json().unwrap();
    let s3_dl_url = download_val["download_url"].as_str().unwrap();
    assert!(s3_dl_url.contains(&document_id));

    // 3. Verify DB explicitly (Explicit ownership check)
    let doc_uuid = uuid::Uuid::parse_str(&document_id).unwrap();
    let flat_mapping: Option<i32> =
        sqlx::query_scalar("SELECT flat_id FROM document_flats WHERE document_id = $1")
            .bind(doc_uuid)
            .fetch_optional(&env.db_pool)
            .await
            .expect("DB query failed");

    assert!(
        flat_mapping.is_some(),
        "Document should be explicitly mapped to a flat in DB"
    );

    // Verify it's mapped to A-101
    let (wing, flat_number): (String, String) =
        sqlx::query_as("SELECT wing, flat_number FROM flats WHERE flat_id = $1")
            .bind(flat_mapping.unwrap())
            .fetch_one(&env.db_pool)
            .await
            .expect("Could not find mapped flat");

    assert_eq!(wing, "A", "Mapped to wrong wing");
    assert_eq!(flat_number, "0101", "Mapped to wrong flat number");
}

/// A `process.reviewer` holder with NO `documents.all.manage` and no document-category
/// "manage" grant must still be able to confirm an upload they started via
/// `/admin/process/upload-url` — regression test for a bug where confirming had to go
/// through the general `admin_confirm_upload` (which requires those unrelated
/// permissions), so a plain reviewer could start an upload but never confirm it, and
/// `save-step` would then reject the document as "not been uploaded yet" forever.
#[tokio::test]
async fn test_process_reviewer_can_confirm_own_process_upload_without_document_permissions() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    quick_create_user(&env, &admin_jwt, "reviewee3@test.com", "A", "101");
    let res_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "reviewee3@test.com", "password123")
    );
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap();
    let pid = resp.into_json::<serde_json::Value>().unwrap()["process_id"]
        .as_str()
        .unwrap()
        .to_string();

    // A scoped designation holding exactly `process.reviewer` — nothing else.
    quick_create_designation(&env, &admin_jwt, "upload_reviewer", "Upload Reviewer");
    quick_toggle_designation_permission(
        &env,
        &admin_jwt,
        "upload_reviewer",
        "process.reviewer",
        true,
    );
    let reviewer_id = quick_create_user(&env, &admin_jwt, "reviewer3@test.com", "B", "202");
    let reviewer_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "reviewer3@test.com", "password123")
    );
    quick_assign_designation(&env, &admin_jwt, reviewer_id, "upload_reviewer");

    let url_resp = ureq::post(&format!("{}/api/v1/admin/process/upload-url", url))
        .set("Authorization", &reviewer_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "title": "Reviewer Upload",
            "filename": "reviewer_upload.pdf",
            "content_type": "application/pdf"
        }))
        .expect("scoped reviewer should be able to start the upload");
    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"reviewer pdf content bytes")
        .expect("S3 PUT upload failed");

    // Sanity: the general admin document confirm-upload endpoint is NOT usable by this
    // reviewer — proves the permission mismatch this endpoint exists to route around.
    let general_confirm = ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &reviewer_auth)
        .send_json(serde_json::json!({ "document_id": document_id }));
    assert!(
        general_confirm.is_err(),
        "a plain process.reviewer must NOT be able to use the document-category-gated confirm endpoint"
    );

    // The process-scoped confirm endpoint must succeed for the same reviewer.
    let confirm_resp = ureq::post(&format!("{}/api/v1/admin/process/confirm-upload", url))
        .set("Authorization", &reviewer_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "document_id": document_id
        }))
        .expect("process-scoped confirm-upload should succeed for the reviewer who started it");
    let confirm_val: serde_json::Value = confirm_resp.into_json().unwrap();
    assert_eq!(confirm_val["status"].as_str().unwrap(), "confirmed");

    // And the now-confirmed document must actually be usable in save-step.
    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "text_value": null, "document_id": document_id}]
        }))
        .expect("save-step should accept the now-confirmed document");
}

fn start_template_one_process(url: &str, auth: &str) -> String {
    ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", auth)
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap()
        .into_json::<serde_json::Value>()
        .unwrap()["process_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn reviewer_uploads_confirmed_document(url: &str, reviewer_auth: &str, pid: &str) -> String {
    let url_val: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/process/upload-url", url))
            .set("Authorization", reviewer_auth)
            .send_json(serde_json::json!({
                "process_id": pid,
                "title": "Reviewer Upload",
                "filename": "reviewer_upload.pdf",
                "content_type": "application/pdf"
            }))
            .expect("reviewer upload-url failed")
            .into_json()
            .unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();
    ureq::put(url_val["upload_url"].as_str().unwrap())
        .set("Content-Type", "application/pdf")
        .send_bytes(b"reviewer pdf content bytes")
        .expect("S3 PUT upload failed");
    ureq::post(&format!("{}/api/v1/admin/process/confirm-upload", url))
        .set("Authorization", reviewer_auth)
        .send_json(serde_json::json!({"process_id": pid, "document_id": document_id}))
        .expect("reviewer confirm-upload failed");
    document_id
}

fn save_document_step(url: &str, auth: &str, pid: &str, document_id: &str) -> Result<(), u16> {
    let resp = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 3,
            "fields": [{"template_field_id": 101, "text_value": null, "document_id": document_id}]
        }));
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

/// A reviewer with only `process.reviewer` (no document permissions) must be able to
/// attach a confirmed upload via `save-step` themselves — including a colleague's
/// upload — but not a deleted one, nor one belonging to a different flat.
#[tokio::test]
async fn test_process_reviewer_can_attach_flat_document_in_save_step() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "attach_res1@test.com", "A", "101");
    let res1_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "attach_res1@test.com", "password123")
    );
    quick_create_user(&env, &admin_jwt, "attach_res2@test.com", "A", "102");
    let res2_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "attach_res2@test.com", "password123")
    );
    let pid1 = start_template_one_process(&url, &res1_auth);
    let pid2 = start_template_one_process(&url, &res2_auth);

    quick_create_designation(&env, &admin_jwt, "attach_reviewer", "Attach Reviewer");
    quick_toggle_designation_permission(
        &env,
        &admin_jwt,
        "attach_reviewer",
        "process.reviewer",
        true,
    );
    let uploader_id = quick_create_user(&env, &admin_jwt, "attach_rev1@test.com", "B", "201");
    let uploader_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "attach_rev1@test.com", "password123")
    );
    quick_assign_designation(&env, &admin_jwt, uploader_id, "attach_reviewer");
    let other_id = quick_create_user(&env, &admin_jwt, "attach_rev2@test.com", "B", "202");
    let other_auth = format!(
        "Bearer {}",
        quick_onboard_user(&env, "attach_rev2@test.com", "password123")
    );
    quick_assign_designation(&env, &admin_jwt, other_id, "attach_reviewer");

    let doc = reviewer_uploads_confirmed_document(&url, &uploader_auth, &pid1);

    save_document_step(&url, &uploader_auth, &pid1, &doc)
        .expect("the uploading reviewer should be able to attach their own upload");
    save_document_step(&url, &other_auth, &pid1, &doc)
        .expect("any reviewer on the process should be able to attach the flat's document");
    let wrong_flat = save_document_step(&url, &uploader_auth, &pid2, &doc);
    assert!(
        wrong_flat == Err(400),
        "a reviewer must not attach a document to a process on a different flat"
    );

    let doc_uuid = uuid::Uuid::parse_str(&doc).unwrap();
    sqlx::query("UPDATE documents SET is_deleted = TRUE WHERE document_id = $1")
        .bind(doc_uuid)
        .execute(&env.db_pool)
        .await
        .unwrap();
    let deleted = save_document_step(&url, &uploader_auth, &pid1, &doc);
    assert!(
        deleted == Err(400),
        "a soft-deleted document must not be attachable"
    );

    sqlx::query("UPDATE documents SET is_deleted = FALSE WHERE document_id = $1")
        .bind(doc_uuid)
        .execute(&env.db_pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_admin_add_note_standalone() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    // 1. Admin adds an internal note
    let resp = ureq::post(&format!("{}/api/v1/admin/process/add-note", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "process_id": pid,
            "template_step_id": 2,
            "comment": "Admin standalone internal note test."
        }))
        .expect("Admin add note failed");

    assert_eq!(resp.status(), 200);

    // 2. Fetch process state as admin - should see the note
    let admin_state_resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .expect("Admin get state failed");

    let admin_val: serde_json::Value = admin_state_resp.into_json().unwrap();
    let admin_comments = admin_val["admin_comments"].as_array().unwrap();
    assert_eq!(admin_comments.len(), 1);
    assert_eq!(
        admin_comments[0]["comment"].as_str().unwrap(),
        "Admin standalone internal note test."
    );
    assert_eq!(admin_comments[0]["template_step_id"].as_i64().unwrap(), 2);

    // 3. Fetch process state as resident - should NOT see the note
    let res_state_resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .expect("Resident get state failed");

    let res_val: serde_json::Value = res_state_resp.into_json().unwrap();
    let res_comments = res_val["admin_comments"].as_array().unwrap();
    assert!(
        res_comments.is_empty(),
        "Resident should not see admin comments"
    );
}
