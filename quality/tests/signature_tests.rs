use quality::{
    quick_assign_designation, quick_create_designation, quick_create_user, quick_onboard_user,
    quick_setup_admin, quick_toggle_designation_permission, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

/// Upload a document via the resident presigned-URL flow.
fn upload_doc(url: &str, user_auth: &str, filename: &str) -> String {
    let url_resp = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", user_auth)
        .send_json(serde_json::json!({
            "title": filename,
            "filename": filename,
            "content_type": "application/pdf"
        }))
        .expect("upload-url request failed");
    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"fake pdf content bytes")
        .expect("S3 PUT upload failed");

    ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", user_auth)
        .send_json(serde_json::json!({"document_id": document_id}))
        .expect("confirm-upload request failed");

    document_id
}

/// Upload a document via the admin process-upload flow (used for field 112, filled_by
/// admin) and confirm it through the admin documents endpoint.
fn upload_doc_as_admin(url: &str, admin_auth: &str, process_id: &str, filename: &str) -> String {
    let url_resp = ureq::post(&format!("{}/api/v1/admin/process/upload-url", url))
        .set("Authorization", admin_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "title": filename,
            "filename": filename,
            "content_type": "application/pdf"
        }))
        .expect("admin process upload-url request failed");
    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = url_val["document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"fake pdf content bytes")
        .expect("S3 PUT upload failed");

    ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", admin_auth)
        .send_json(serde_json::json!({"document_id": document_id}))
        .expect("admin confirm-upload request failed");

    document_id
}

/// Signs a document via the presigned upload-url/confirm pair — the real flow (bytes
/// go straight to S3, matching every other upload path in this app; this server runs
/// on Lambda, which caps request payload well below what a signed file can be).
fn sign_document(
    url: &str,
    signer_auth: &str,
    document_id: &str,
) -> Result<serde_json::Value, Box<ureq::Error>> {
    let url_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/replace-upload-url",
        url
    ))
    .set("Authorization", signer_auth)
    .send_json(serde_json::json!({"document_id": document_id, "content_type": "application/pdf"}))
    .map_err(Box::new)?;
    let url_val: serde_json::Value = url_resp.into_json().unwrap();
    let new_document_id = url_val["new_document_id"].as_str().unwrap().to_string();
    let upload_url = url_val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"signed pdf content bytes")
        .map_err(Box::new)?;

    let confirm_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/replace-confirm-upload",
        url
    ))
    .set("Authorization", signer_auth)
    .send_json(serde_json::json!({"document_id": document_id, "new_document_id": new_document_id}))
    .map_err(Box::new)?;
    Ok(confirm_resp.into_json().unwrap())
}

fn save_step(
    url: &str,
    caller_auth: &str,
    process_id: &str,
    template_step_id: i32,
    fields: serde_json::Value,
) {
    let resp = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", caller_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": template_step_id,
            "fields": fields
        }))
        .expect("save-step failed");
    assert!(resp.status() < 300);
}

fn admin_approve(url: &str, admin_auth: &str, process_id: &str, template_step_id: i32) {
    ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", admin_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": template_step_id,
            "is_approved": true
        }))
        .expect("admin review/approve failed");
}

/// Creates a resident + process, and walks it (Steps 1/Step 2 substeps -> Step 2
/// meeting) all the way to Step 3 (NOC Issuance, field 112, `requires_signature`)
/// being Open. Returns (process_id, resident_auth).
fn advance_to_step_8(
    url: &str,
    env: &TestEnv,
    admin_jwt: &str,
    email: &str,
    flat: &str,
) -> (String, String) {
    quick_create_user(env, admin_jwt, email, "SG", flat);
    let res_jwt = quick_onboard_user(env, email, "password123");
    let res_auth = auth(&res_jwt);
    let admin_auth = auth(admin_jwt);

    let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": "tenant_onboarding_v1"}))
        .unwrap()
        .into_json()
        .unwrap();
    let pid = resp["process_id"].as_str().unwrap().to_string();

    let filler_doc = upload_doc(url, &res_auth, "filler.pdf");

    save_step(url, &res_auth, &pid, 3, serde_json::json!(
        [101, 102, 103, 105, 106].map(|fid| serde_json::json!({"template_field_id": fid, "text_value": null, "document_id": filler_doc}))
    ));
    save_step(
        url,
        &res_auth,
        &pid,
        4,
        serde_json::json!([{"template_field_id": 107, "text_value": null, "document_id": filler_doc}]),
    );
    save_step(
        url,
        &res_auth,
        &pid,
        5,
        serde_json::json!([{"template_field_id": 108, "text_value": null, "document_id": filler_doc}]),
    );
    save_step(
        url,
        &res_auth,
        &pid,
        6,
        serde_json::json!([
            {"template_field_id": 109, "text_value": null, "document_id": filler_doc},
            {"template_field_id": 110, "text_value": null, "document_id": filler_doc}
        ]),
    );

    admin_approve(url, &admin_auth, &pid, 2);

    save_step(
        url,
        &admin_auth,
        &pid,
        7,
        serde_json::json!([{"template_field_id": 111, "text_value": "Meeting held", "document_id": null}]),
    );
    admin_approve(url, &admin_auth, &pid, 7);

    (pid, res_auth)
}

/// Seed a designation holding exactly the given permission codes and assign it to a
/// fresh resident, returning that resident's JWT.
fn seeded_resident(
    env: &TestEnv,
    admin_jwt: &str,
    tag: &str,
    flat_number: &str,
    permission_codes: &[&str],
) -> String {
    let designation_code = format!("sig_{}", tag);
    let email = format!("{}@sigtest.test", tag);

    quick_create_designation(env, admin_jwt, &designation_code, &designation_code);
    for perm in permission_codes {
        quick_toggle_designation_permission(env, admin_jwt, &designation_code, perm, true);
    }

    let rid = quick_create_user(env, admin_jwt, &email, "SG", flat_number);
    let res_jwt = quick_onboard_user(env, &email, "password123");
    quick_assign_designation(env, admin_jwt, rid, &designation_code);
    res_jwt
}

async fn is_step_completed(env: &TestEnv, process_id: &str, template_step_id: i32) -> bool {
    let pid = uuid::Uuid::parse_str(process_id).unwrap();
    sqlx::query_scalar::<_, bool>(
        "SELECT is_completed FROM process_steps WHERE process_id = $1 AND template_step_id = $2",
    )
    .bind(pid)
    .bind(template_step_id)
    .fetch_one(&env.db_pool)
    .await
    .unwrap()
}

fn find_field(state: &serde_json::Value, step_id: i32, field_id: i32) -> &serde_json::Value {
    for step in state["steps"].as_array().unwrap() {
        if step["template_step_id"] == step_id {
            for field in step["fields"].as_array().unwrap() {
                if field["template_field_id"] == field_id {
                    return field;
                }
            }
        }
        for sub in step["substeps"].as_array().unwrap() {
            if sub["template_step_id"] == step_id {
                for field in sub["fields"].as_array().unwrap() {
                    if field["template_field_id"] == field_id {
                        return field;
                    }
                }
            }
        }
    }
    panic!(
        "field {}/{} not found in process state response",
        step_id, field_id
    );
}

#[tokio::test]
async fn test_auto_provisioning_creates_requirement_and_blocks_step() {
    let env = setup_api_test_env(3200).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, _res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig1@test.com", "1");

    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": doc_id}]),
    );

    let requirement: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/for-document",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"document_id": doc_id}))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(requirement["status"], "PENDING");
    assert_eq!(requirement["document_id"], doc_id);

    assert!(
        !is_step_completed(&env, &pid, 8).await,
        "step must not complete while its document is awaiting signature"
    );
}

#[tokio::test]
async fn test_queue_visibility_requires_sign_permission() {
    let env = setup_api_test_env(3201).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, _res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig2@test.com", "2");
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": doc_id}]),
    );

    let signer_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "signer2",
        "3",
        &["documents.signatures.sign"],
    );
    let queue: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
            .set("Authorization", &auth(&signer_jwt))
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert!(queue.iter().any(|r| r["document_id"] == doc_id));

    let nobody_jwt = seeded_resident(&env, &admin_jwt, "nosign2", "4", &[]);
    let err = ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
        .set("Authorization", &auth(&nobody_jwt))
        .send_json(serde_json::json!({}));
    assert!(err.is_err());
}

#[tokio::test]
async fn test_signing_creates_new_document_and_updates_field() {
    let env = setup_api_test_env(3202).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig3@test.com", "5");
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": doc_id}]),
    );

    let signer_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "signer3",
        "6",
        &["documents.signatures.sign"],
    );
    let signer_auth = auth(&signer_jwt);

    let confirmed = sign_document(&url, &signer_auth, &doc_id).expect("sign should succeed");
    assert_eq!(confirmed["status"], "COMPLETED");
    let new_doc_id = confirmed["signed_document_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new_doc_id, doc_id);

    let original: serde_json::Value = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(original["document"]["document_id"], doc_id);

    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap()
        .into_json()
        .unwrap();
    let field = find_field(&state, 8, 112);
    assert_eq!(field["value"]["document_id"], new_doc_id);

    assert!(is_step_completed(&env, &pid, 8).await);

    let for_new: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/for-document",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({"document_id": new_doc_id}))
    .unwrap()
    .into_json()
    .unwrap();
    assert!(for_new.is_null());
}

#[tokio::test]
async fn test_cannot_sign_twice_second_call_rejected() {
    let env = setup_api_test_env(3203).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, _res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig4@test.com", "7");
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": doc_id}]),
    );

    let signer_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "signer4",
        "8",
        &["documents.signatures.sign"],
    );
    let signer_auth = auth(&signer_jwt);

    let first = sign_document(&url, &signer_auth, &doc_id);
    assert!(first.is_ok());

    let second = sign_document(&url, &signer_auth, &doc_id);
    assert!(
        second.is_err(),
        "a requirement already COMPLETED must reject a second sign"
    );
}

#[tokio::test]
async fn test_cancel_unblocks_step_without_signing() {
    let env = setup_api_test_env(3204).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, _res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig5@test.com", "9");
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": doc_id}]),
    );
    assert!(!is_step_completed(&env, &pid, 8).await);

    ureq::post(&format!("{}/api/v1/admin/documents/signatures/cancel", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"document_id": doc_id}))
        .expect("cancel should succeed for documents.signatures.manage holder");

    assert!(
        is_step_completed(&env, &pid, 8).await,
        "cancelling the requirement must unblock the step immediately, with no signing"
    );
}

#[tokio::test]
async fn test_self_healing_stale_requirement_does_not_affect_current_field() {
    let env = setup_api_test_env(3205).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig6@test.com", "10");

    let old_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc_v1.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": old_doc_id}]),
    );

    let new_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc_v2.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": new_doc_id}]),
    );

    let signer_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "signer6",
        "11",
        &["documents.signatures.sign"],
    );
    let signer_auth = auth(&signer_jwt);

    sign_document(&url, &signer_auth, &old_doc_id)
        .expect("signing a stale requirement must still succeed as a request");

    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"process_id": pid}))
        .unwrap()
        .into_json()
        .unwrap();
    let field = find_field(&state, 8, 112);
    assert_eq!(field["value"]["document_id"], new_doc_id);

    assert!(!is_step_completed(&env, &pid, 8).await);
}

#[tokio::test]
async fn test_permission_boundaries_sign_vs_manage() {
    let env = setup_api_test_env(3206).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (pid, _res_auth) = advance_to_step_8(&url, &env, &admin_jwt, "sig7@test.com", "12");
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(
        &url,
        &admin_auth,
        &pid,
        8,
        serde_json::json!([{"template_field_id": 112, "text_value": null, "document_id": doc_id}]),
    );

    let manage_only_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "manageonly7",
        "13",
        &["documents.signatures.manage"],
    );
    let manage_auth = auth(&manage_only_jwt);

    assert!(
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
            .set("Authorization", &manage_auth)
            .send_json(serde_json::json!({}))
            .is_err(),
        "documents.signatures.manage alone must not grant queue access"
    );
    assert!(
        ureq::post(&format!(
            "{}/api/v1/admin/documents/signatures/for-document",
            url
        ))
        .set("Authorization", &manage_auth)
        .send_json(serde_json::json!({"document_id": doc_id}))
        .is_ok(),
        "documents.signatures.manage must grant read access"
    );

    let sign_only_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "signonly7",
        "14",
        &["documents.signatures.sign"],
    );
    let sign_auth = auth(&sign_only_jwt);
    assert!(
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/cancel", url))
            .set("Authorization", &sign_auth)
            .send_json(serde_json::json!({"document_id": doc_id}))
            .is_err(),
        "documents.signatures.sign alone must not grant cancel access"
    );
}
