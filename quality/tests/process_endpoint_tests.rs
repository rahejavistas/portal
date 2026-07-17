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
        .send_json(serde_json::json!({"template_id": "tenant_onboarding_v1"}))
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
    let env = setup_api_test_env(3110).await;
    let url = env.server_url();
    let (pid, res_auth, _) = setup_process(&url, &env);

    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();

    assert_eq!(val["template_id"].as_str().unwrap(), "tenant_onboarding_v1");
    assert!(!val["is_completed"].as_bool().unwrap());
    assert_eq!(val["steps"].as_array().unwrap().len(), 5);
    // First step should be OPEN, second should be LOCKED
    let steps = val["steps"].as_array().unwrap();
    assert_eq!(steps[0]["status"].as_str().unwrap(), "OPEN");
    assert_eq!(steps[1]["status"].as_str().unwrap(), "LOCKED");
}

#[tokio::test]
async fn test_state_creates_new_process_each_time() {
    let env = setup_api_test_env(3111).await;
    let url = env.server_url();
    let (pid1, res_auth, _) = setup_process(&url, &env);

    // Call again with template_id — should create a new process as per recent changes
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": "tenant_onboarding_v1"}))
        .unwrap();
    let val: serde_json::Value = resp.into_json().unwrap();
    assert_ne!(val["process_id"].as_str().unwrap(), pid1);
}

#[tokio::test]
async fn test_state_admin_can_start_own_process_without_process_id() {
    // A user holding process.reviewer (e.g. super_admin) can still start their own
    // process by template_id — reviewing others' steps doesn't preclude having one's own.
    let env = setup_api_test_env(3112).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(serde_json::json!({"template_id": "tenant_onboarding_v1"}));
    assert!(resp.is_ok());
}

#[tokio::test]
async fn test_state_invalid_process_id_returns_404() {
    let env = setup_api_test_env(3113).await;
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
    let env = setup_api_test_env(3114).await;
    let url = env.server_url();
    let (pid, res_auth, _admin_auth) = setup_process(&url, &env);

    let doc_id = upload_doc(&url, &res_auth, &pid, "test.pdf");
    assert!(Uuid::parse_str(&doc_id).is_ok());
}

#[tokio::test]
async fn test_upload_rejects_empty_file() {
    let env = setup_api_test_env(3115).await;
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
    let env = setup_api_test_env(3116).await;
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
    let env = setup_api_test_env(3117).await;
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
    let env = setup_api_test_env(3125).await;
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
    let env = setup_api_test_env(3118).await;
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
    let env = setup_api_test_env(3119).await;
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
    let env = setup_api_test_env(3120).await;
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
    let env = setup_api_test_env(3121).await;
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
    let env = setup_api_test_env(3122).await;
    let url = env.server_url();
    let (pid, res_auth, admin_auth) = setup_process(&url, &env);

    let doc_id = upload_doc(&url, &res_auth, &pid, "test.pdf");

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
    let env = setup_api_test_env(3123).await;
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
    let env = setup_api_test_env(3177).await;
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
        .send_json(serde_json::json!({"template_id": "tenant_onboarding_v1"}))
        .unwrap();
    let pid = resp.into_json::<serde_json::Value>().unwrap()["process_id"]
        .as_str()
        .unwrap()
        .to_string();
    let doc_id = upload_doc(&url, &res_auth, &pid, "reviewee_doc.pdf");

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

// ============================================================
// Access Control
// ============================================================

#[tokio::test]
async fn test_user_cannot_access_other_users_process() {
    let env = setup_api_test_env(3124).await;
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
        .send_json(serde_json::json!({"template_id": "tenant_onboarding_v1"}))
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
    let env = setup_api_test_env(3133).await;
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

#[tokio::test]
async fn test_admin_add_note_standalone() {
    let env = setup_api_test_env(3134).await;
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
