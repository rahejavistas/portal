//! Coverage for the e-signature feature: a process reviewer attaches signers (from the
//! self-service signer directory) to a document, IN ORDER — earlier signers must
//! complete before later ones become actionable, and the step stays blocked until
//! every attached signer has signed.

use quality::{
    quick_assign_designation, quick_create_designation, quick_create_user, quick_onboard_user,
    quick_revoke_designation, quick_setup_admin, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

/// Signers can only ever be attached while a step is still `Open` (see
/// `admin_add_document_signers`), and a document field completes its step immediately
/// on submission unless it declares `requires_signature: true` — so any field a test
/// wants to attach signers to must set it, matching how the real UI gates the "Add
/// Signers" button the same way.
fn simple_template_draft() -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Sig Test Template",
        "substeps": [
            {
                "name": "Doc Stage",
                "fields": [
                    { "name": "NOC Document", "type": "document", "filled_by": "admin", "requires_signature": true }
                ]
            }
        ]
    })
}

/// Like `simple_template_draft` but the field also declares `required_signs_from_designations`
/// (array order matters — see `add-signers`' ordering enforcement).
fn template_draft_with_required_designations(codes: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Sig Test Template With Designations",
        "substeps": [
            {
                "name": "Doc Stage",
                "fields": [
                    { "name": "NOC Document", "type": "document", "filled_by": "admin", "requires_signature": true, "required_signs_from_designations": codes }
                ]
            }
        ]
    })
}

/// Like `simple_template_draft` but the field also declares `requires_signature: true`
/// — the step must not complete until at least one signer is attached AND every
/// attached signer has signed, regardless of whether the document itself is present.
/// `designations` is passed straight through as `required_signs_from_designations`
/// (empty = no restriction, any registered signer is eligible).
fn template_draft_requires_signature(designations: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Sig Test Template Requires Signature",
        "substeps": [
            {
                "name": "Doc Stage",
                "fields": [
                    {
                        "name": "NOC Document", "type": "document", "filled_by": "admin",
                        "requires_signature": true,
                        "required_signs_from_designations": designations
                    }
                ]
            }
        ]
    })
}

/// Creates a template with a single admin-filled document field, activates it, and
/// returns (template_id, stage_step_id, field_id).
fn save_template(url: &str, jwt: &str) -> (i32, i32, i32) {
    save_template_draft(url, jwt, simple_template_draft())
}

/// Same as `save_template` but for a caller-provided draft (e.g. one with
/// `required_signs_from_designations` set on the field).
fn save_template_draft(url: &str, jwt: &str, draft: serde_json::Value) -> (i32, i32, i32) {
    let resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
            .set("Authorization", &auth(jwt))
            .send_json(draft)
            .expect("template save failed")
            .into_json()
            .unwrap();
    let version_id = resp["version_id"].as_i64().unwrap() as i32;

    let templates: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/process-templates/list", url))
            .set("Authorization", &auth(jwt))
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    let template_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(version_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;

    ureq::post(&format!("{}/api/v1/admin/process-templates/activate", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "template_id": template_id, "version_id": version_id }))
        .expect("activate failed");

    let tree: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/get-version",
        url
    ))
    .set("Authorization", &auth(jwt))
    .send_json(serde_json::json!({ "version_id": version_id }))
    .unwrap()
    .into_json()
    .unwrap();
    let stage = &tree["substeps"][0];
    let stage_step_id = stage["step_id"].as_i64().unwrap() as i32;
    let field_id = stage["fields"][0]["field_id"].as_i64().unwrap() as i32;

    (template_id, stage_step_id, field_id)
}

fn start_process(url: &str, resident_jwt: &str, template_id: i32) -> String {
    let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(resident_jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .unwrap()
        .into_json()
        .unwrap();
    resp["process_id"].as_str().unwrap().to_string()
}

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

fn save_step(
    url: &str,
    caller_auth: &str,
    process_id: &str,
    template_step_id: i32,
    field_id: i32,
    document_id: &str,
) {
    let resp = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", caller_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": template_step_id,
            "fields": [{"template_field_id": field_id, "text_value": null, "document_id": document_id}]
        }))
        .expect("save-step failed");
    assert!(resp.status() < 300);
}

/// Creates a fresh designation named `sig_<tag>` and assigns it to a new resident —
/// holding ANY designation is this app's whole eligibility bar for signing (see
/// `authentication::User::has_any_designation`), no dedicated permission involved.
/// Then self-registers them as a signer (mirrors the real Profile-page flow). Returns
/// (jwt, signer_id, user_id, designation_code) — the designation code is handy as-is
/// for `required_signs_from_designations` tests, since it's already the one and only
/// designation this signer holds.
fn make_signer(
    env: &TestEnv,
    admin_jwt: &str,
    tag: &str,
    flat_number: &str,
) -> (String, i32, i32, String) {
    let designation_code = format!("sig_{}", tag);
    let email = format!("{}@sigtest.test", tag);
    quick_create_designation(env, admin_jwt, &designation_code, &designation_code);
    let uid = quick_create_user(env, admin_jwt, &email, "SG", flat_number);
    let jwt = quick_onboard_user(env, &email, "password123");
    quick_assign_designation(env, admin_jwt, uid, &designation_code);

    let url = env.server_url();
    let signer: serde_json::Value = ureq::post(&format!("{}/api/v1/profile/signer-info/save", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "name": tag, "phone": "9876543210", "email": email }))
        .expect("self-service signer registration should succeed")
        .into_json()
        .unwrap();
    let signer_id = signer["signer_id"].as_i64().unwrap() as i32;

    (jwt, signer_id, uid, designation_code)
}

fn add_signers(
    url: &str,
    admin_auth: &str,
    process_id: &str,
    step_id: i32,
    field_id: i32,
    document_id: &str,
    signer_ids: &[i32],
) -> Result<Vec<serde_json::Value>, u16> {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/process/documents/add-signers",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "process_id": process_id,
        "template_step_id": step_id,
        "template_field_id": field_id,
        "document_id": document_id,
        "signer_ids": signer_ids
    }));
    match resp {
        Ok(r) => {
            let v: serde_json::Value = r.into_json().unwrap();
            Ok(v["requirements"].as_array().unwrap().clone())
        }
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

/// Fetches process state as `caller_auth` — used to check read-access authorization
/// independent of any particular field.
fn get_process_state(
    url: &str,
    caller_auth: &str,
    process_id: &str,
) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", caller_auth)
        .send_json(serde_json::json!({ "process_id": process_id }));
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn requirement_id_for_signer(requirements: &[serde_json::Value], signer_id: i32) -> i64 {
    requirements
        .iter()
        .find(|r| r["signer_id"].as_i64() == Some(signer_id as i64))
        .unwrap_or_else(|| panic!("no requirement for signer {}", signer_id))["requirement_id"]
        .as_i64()
        .unwrap()
}

/// Signs a requirement via the presigned upload-url/confirm pair — the real flow.
fn sign_requirement(
    url: &str,
    signer_auth: &str,
    requirement_id: i64,
) -> Result<serde_json::Value, Box<ureq::Error>> {
    let url_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/replace-upload-url",
        url
    ))
    .set("Authorization", signer_auth)
    .send_json(
        serde_json::json!({"requirement_id": requirement_id, "content_type": "application/pdf"}),
    )
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
    .send_json(
        serde_json::json!({"requirement_id": requirement_id, "new_document_id": new_document_id}),
    )
    .map_err(Box::new)?;
    Ok(confirm_resp.into_json().unwrap())
}

/// Same as `sign_requirement` but also returns the freshly-created signed-replacement
/// document id, so callers can check whether the field actually got repointed at it.
fn sign_requirement_with_new_doc(
    url: &str,
    signer_auth: &str,
    requirement_id: i64,
) -> Result<(serde_json::Value, String), Box<ureq::Error>> {
    let url_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/replace-upload-url",
        url
    ))
    .set("Authorization", signer_auth)
    .send_json(
        serde_json::json!({"requirement_id": requirement_id, "content_type": "application/pdf"}),
    )
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
    .send_json(
        serde_json::json!({"requirement_id": requirement_id, "new_document_id": new_document_id}),
    )
    .map_err(Box::new)?;
    Ok((confirm_resp.into_json().unwrap(), new_document_id))
}

/// Recursively searches a `/process/state` response for `template_field_id` and
/// returns its current `value.document_id`, if any.
fn field_document_id(state: &serde_json::Value, template_field_id: i32) -> Option<String> {
    field_json(state, template_field_id)
        .and_then(|f| f["value"]["document_id"].as_str().map(|s| s.to_string()))
}

/// Recursively searches a `/process/state` response for `template_field_id` and
/// returns its `signature_status` array (empty if the field has no such entries).
fn field_signature_statuses(
    state: &serde_json::Value,
    template_field_id: i32,
) -> Vec<serde_json::Value> {
    field_json(state, template_field_id)
        .and_then(|f| f["signature_status"].as_array().cloned())
        .unwrap_or_default()
}

fn field_json(state: &serde_json::Value, template_field_id: i32) -> Option<serde_json::Value> {
    fn search(step: &serde_json::Value, template_field_id: i32) -> Option<serde_json::Value> {
        if let Some(fields) = step["fields"].as_array() {
            for f in fields {
                if f["template_field_id"].as_i64() == Some(template_field_id as i64) {
                    return Some(f.clone());
                }
            }
        }
        if let Some(substeps) = step["substeps"].as_array() {
            for s in substeps {
                if let Some(found) = search(s, template_field_id) {
                    return Some(found);
                }
            }
        }
        None
    }
    state["steps"]
        .as_array()
        .and_then(|steps| steps.iter().find_map(|s| search(s, template_field_id)))
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

#[tokio::test]
async fn test_add_signers_creates_ordered_requirements_and_blocks_step() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(&env, &admin_jwt, "resident1@sigtest.test", "SG", "201");
    let res_jwt = quick_onboard_user(&env, "resident1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (_jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "sig1a", "202");
    let (_jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "sig1b", "203");

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1, signer2],
    )
    .expect("add signers should succeed");
    assert_eq!(requirements.len(), 2);
    let r1 = requirements
        .iter()
        .find(|r| r["signer_id"] == signer1)
        .unwrap();
    let r2 = requirements
        .iter()
        .find(|r| r["signer_id"] == signer2)
        .unwrap();
    assert_eq!(r1["sign_order"], 1);
    assert_eq!(r2["sign_order"], 2);
    assert_eq!(r1["status"], "PENDING");
    assert_eq!(r2["status"], "PENDING");

    assert!(
        !is_step_completed(&env, &pid, step_id).await,
        "step must not complete while any attached signer hasn't signed"
    );
}

#[tokio::test]
async fn test_signers_must_sign_in_order() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(&env, &admin_jwt, "resident2@sigtest.test", "SG", "204");
    let res_jwt = quick_onboard_user(&env, "resident2@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "sig2a", "205");
    let (jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "sig2b", "206");

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1, signer2],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);
    let req2 = requirement_id_for_signer(&requirements, signer2);

    let out_of_order = sign_requirement(&url, &auth(&jwt2), req2);
    assert!(
        out_of_order.is_err(),
        "the second signer must not be able to act before the first has signed"
    );

    sign_requirement(&url, &auth(&jwt1), req1).expect("first signer should succeed");
    assert!(!is_step_completed(&env, &pid, step_id).await);

    let confirmed = sign_requirement(&url, &auth(&jwt2), req2)
        .expect("second signer should now be able to sign");
    assert_eq!(confirmed["status"], "COMPLETED");

    assert!(
        is_step_completed(&env, &pid, step_id).await,
        "step must complete once every attached signer has signed"
    );
}

#[tokio::test]
async fn test_queue_only_shows_current_turn() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(&env, &admin_jwt, "resident3@sigtest.test", "SG", "207");
    let res_jwt = quick_onboard_user(&env, "resident3@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "sig3a", "208");
    let (jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "sig3b", "209");

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1, signer2],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);

    let queue2_before: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
            .set("Authorization", &auth(&jwt2))
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert!(
        queue2_before.is_empty(),
        "second signer's queue must be empty until it's their turn"
    );

    sign_requirement(&url, &auth(&jwt1), req1).expect("first signer should succeed");

    let queue2_after: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
            .set("Authorization", &auth(&jwt2))
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(
        queue2_after.len(),
        1,
        "second signer's queue now shows their requirement"
    );
}

#[tokio::test]
async fn test_cannot_sign_someone_elses_requirement() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(&env, &admin_jwt, "resident5@sigtest.test", "SG", "212");
    let res_jwt = quick_onboard_user(&env, "resident5@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (_jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "sig5a", "213");
    let (jwt2, _signer2, ..) = make_signer(&env, &admin_jwt, "sig5b", "214");
    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);

    let err = sign_requirement(&url, &auth(&jwt2), req1);
    assert!(
        err.is_err(),
        "a different signer must not be able to sign this requirement"
    );
}

/// A signer's document access is scoped per-requirement, not per-flat or per-category —
/// two entirely separate flats/processes/signers, and a signer attached to one must get
/// 403 trying to view, upload-to, or confirm-upload the other flat's document via every
/// signature endpoint, using the other flat's real requirement_id (not a guess). Also
/// confirms the signing queue never leaks the other flat's item.
#[tokio::test]
async fn test_signer_has_no_access_to_a_different_flats_document() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    // Flat A: its own resident, document, and signer.
    quick_create_user(
        &env,
        &admin_jwt,
        "flat_a_resident@sigtest.test",
        "SG",
        "301",
    );
    let res_a_jwt = quick_onboard_user(&env, "flat_a_resident@sigtest.test", "password123");
    let pid_a = start_process(&url, &res_a_jwt, template_id);
    let doc_a = upload_doc_as_admin(&url, &admin_auth, &pid_a, "flat_a.pdf");
    save_step(&url, &admin_auth, &pid_a, step_id, field_id, &doc_a);
    let (_jwt_a, signer_a, ..) = make_signer(&env, &admin_jwt, "flatasign", "302");
    let reqs_a = add_signers(
        &url,
        &admin_auth,
        &pid_a,
        step_id,
        field_id,
        &doc_a,
        &[signer_a],
    )
    .unwrap();
    let req_a = requirement_id_for_signer(&reqs_a, signer_a);

    // Flat B: a completely separate resident, document, and signer.
    quick_create_user(
        &env,
        &admin_jwt,
        "flat_b_resident@sigtest.test",
        "SG",
        "303",
    );
    let res_b_jwt = quick_onboard_user(&env, "flat_b_resident@sigtest.test", "password123");
    let pid_b = start_process(&url, &res_b_jwt, template_id);
    let doc_b = upload_doc_as_admin(&url, &admin_auth, &pid_b, "flat_b.pdf");
    save_step(&url, &admin_auth, &pid_b, step_id, field_id, &doc_b);
    let (jwt_b, signer_b, ..) = make_signer(&env, &admin_jwt, "flatbsign", "304");
    let reqs_b = add_signers(
        &url,
        &admin_auth,
        &pid_b,
        step_id,
        field_id,
        &doc_b,
        &[signer_b],
    )
    .unwrap();
    let req_b = requirement_id_for_signer(&reqs_b, signer_b);

    // Flat B's signer must not be able to view Flat A's document to sign.
    let view_other_flat = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/document-download-url",
        url
    ))
    .set("Authorization", &auth(&jwt_b))
    .send_json(serde_json::json!({ "requirement_id": req_a }));
    assert!(
        view_other_flat.is_err(),
        "a signer must not be able to view another flat's document to sign, even with its real requirement_id"
    );

    // Nor start an upload against it.
    let upload_other_flat = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/replace-upload-url",
        url
    ))
    .set("Authorization", &auth(&jwt_b))
    .send_json(serde_json::json!({ "requirement_id": req_a, "content_type": "application/pdf" }));
    assert!(
        upload_other_flat.is_err(),
        "a signer must not be able to start a signed-replacement upload for another flat's requirement"
    );

    // Nor confirm one, even with a document_id they don't actually control.
    let confirm_other_flat = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/replace-confirm-upload",
        url
    ))
    .set("Authorization", &auth(&jwt_b))
    .send_json(serde_json::json!({ "requirement_id": req_a, "new_document_id": doc_a }));
    assert!(
        confirm_other_flat.is_err(),
        "a signer must not be able to confirm a signed-replacement upload for another flat's requirement"
    );

    // Flat B signer's own queue must show only Flat B's item, never Flat A's.
    let queue: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
            .set("Authorization", &auth(&jwt_b))
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0]["requirement_id"], req_b);
    assert!(
        queue.iter().all(|r| r["requirement_id"] != req_a),
        "a signer's queue must never include another flat's requirement"
    );
}

/// There is no delete for a signer — only deactivate, which is reversible and never
/// touches anything already in progress. Deactivating a signer with a PENDING
/// requirement must leave that requirement (and the signer row itself) completely
/// untouched — they can still complete it — and simply stops them from being offered
/// for NEW attachments.
#[tokio::test]
async fn test_deactivating_signer_does_not_affect_pending_requirement() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(&env, &admin_jwt, "resident_del1@sigtest.test", "SG", "241");
    let res_jwt = quick_onboard_user(&env, "resident_del1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "sigdel1", "242");
    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);

    ureq::post(&format!("{}/api/v1/admin/signers/set-active", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "signer_id": signer1, "is_active": false }))
        .expect("deactivate should succeed even with a pending requirement");

    let signer_still_exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM document_signers WHERE signer_id = $1")
            .bind(signer1)
            .fetch_one(&env.db_pool)
            .await
            .unwrap();
    assert_eq!(
        signer_still_exists, 1,
        "deactivate must never remove the signer row"
    );

    sign_requirement(&url, &auth(&jwt1), req1)
        .expect("a deactivated signer can still complete a requirement already waiting on them");

    assert_eq!(
        requirement_status(&env, req1).await,
        "COMPLETED",
        "deactivation must not have blocked or altered the pending requirement"
    );
}

#[tokio::test]
async fn test_self_service_signer_info_requires_any_designation() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "noresident@sigtest.test", "SG", "215");
    let jwt = quick_onboard_user(&env, "noresident@sigtest.test", "password123");

    let resp = ureq::post(&format!("{}/api/v1/profile/signer-info/save", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "name": "No Designation", "phone": "9999999999", "email": "noresident@sigtest.test" }));
    assert!(
        resp.is_err(),
        "a plain resident holding no designation at all must not be able to register as a signer"
    );
}

/// `super_admin` doesn't count on its own — it's an abstract "holds every permission"
/// role, not a real person realistically picked as a document signer. A plain
/// super_admin account (no other designation) must not be sign-eligible just by
/// virtue of being admin — otherwise every super_admin sees a "Signing Queue" they can
/// never actually use.
#[tokio::test]
async fn test_super_admin_alone_is_not_sign_eligible() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let resp = ureq::post(&format!("{}/api/v1/profile/signer-info/save", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({ "name": "Admin", "phone": "9999999999", "email": "superadmin@vistas.com" }));
    assert!(
        resp.is_err(),
        "super_admin alone must not be treated as sign-eligible"
    );
}

/// There's no dedicated signing permission — holding ANY designation (even one with
/// zero permissions of its own) is enough to self-register.
#[tokio::test]
async fn test_self_service_signer_info_allowed_for_any_designation_holder() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_designation(&env, &admin_jwt, "sig_bare", "sig_bare");
    let uid = quick_create_user(&env, &admin_jwt, "bare@sigtest.test", "SG", "216");
    let jwt = quick_onboard_user(&env, "bare@sigtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, uid, "sig_bare");

    let resp = ureq::post(&format!("{}/api/v1/profile/signer-info/save", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "name": "Bare Designation", "phone": "9999999999", "email": "bare@sigtest.test" }));
    assert!(
        resp.is_ok(),
        "holding any designation at all — even one with no permissions — should be enough to self-register"
    );
}

// ==========================================================================
// `required_signs_from_designations` — a template field can scope+order the "Add Signers"
// picker to specific designations. Enforced server-side both when signers are
// attached AND again at the moment of signing (so a designation revoked in between
// invalidates the pending signature).
// ==========================================================================

#[tokio::test]
async fn test_add_signers_rejects_signer_without_required_designation() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (_jwt1, signer1, _uid1, designation1) = make_signer(&env, &admin_jwt, "des1a", "220");
    let (_jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "des1b", "221");

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des1@sigtest.test", "SG", "222");
    let res_jwt = quick_onboard_user(&env, "resident_des1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let rejected = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer2],
    );
    assert_eq!(
        rejected,
        Err(400),
        "signer2 doesn't hold the field's required designation and must be rejected"
    );

    let accepted = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    );
    assert!(
        accepted.is_ok(),
        "signer1 holds the required designation and should be accepted"
    );
}

#[tokio::test]
async fn test_add_signers_rejects_out_of_order_designations() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (_jwt1, signer1, _uid1, designation1) = make_signer(&env, &admin_jwt, "des2a", "223");
    let (_jwt2, signer2, _uid2, designation2) = make_signer(&env, &admin_jwt, "des2b", "224");

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1, &designation2]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des2@sigtest.test", "SG", "225");
    let res_jwt = quick_onboard_user(&env, "resident_des2@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    // Template requires designation1 before designation2 — submitting signer2 (designation2)
    // ahead of signer1 (designation1) must be rejected.
    let rejected = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer2, signer1],
    );
    assert_eq!(
        rejected,
        Err(400),
        "signer order must follow designation order"
    );
}

#[tokio::test]
async fn test_add_signers_accepts_correct_designation_order() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (_jwt1, signer1, _uid1, designation1) = make_signer(&env, &admin_jwt, "des3a", "226");
    let (_jwt2, signer2, _uid2, designation2) = make_signer(&env, &admin_jwt, "des3b", "227");

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1, &designation2]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des3@sigtest.test", "SG", "228");
    let res_jwt = quick_onboard_user(&env, "resident_des3@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1, signer2],
    )
    .expect("designation-ordered signers should be accepted");

    let r1 = requirements
        .iter()
        .find(|r| r["signer_id"] == signer1)
        .unwrap();
    let r2 = requirements
        .iter()
        .find(|r| r["signer_id"] == signer2)
        .unwrap();
    assert_eq!(r1["sign_order"], 1);
    assert_eq!(r2["sign_order"], 2);
}

#[tokio::test]
async fn test_add_signers_allows_any_order_when_designations_tie() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (_jwt1, signer1, _uid1, designation1) = make_signer(&env, &admin_jwt, "des4a", "229");
    let (_jwt2, signer2, uid2, _designation2) = make_signer(&env, &admin_jwt, "des4b", "230");
    // Give signer2 the SAME designation as signer1 too, so both cover the field's one
    // required designation — order between them is unconstrained (a tie).
    quick_assign_designation(&env, &admin_jwt, uid2, &designation1);

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des4@sigtest.test", "SG", "231");
    let res_jwt = quick_onboard_user(&env, "resident_des4@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer2, signer1],
    );
    assert!(
        requirements.is_ok(),
        "signers tied on the same designation rank may be submitted in either order"
    );
}

#[tokio::test]
async fn test_add_signers_rejects_signer_whose_designation_was_already_revoked() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (_jwt1, signer1, uid1, designation1) = make_signer(&env, &admin_jwt, "des5a", "232");
    quick_revoke_designation(&env, &admin_jwt, uid1, &designation1);

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des5@sigtest.test", "SG", "233");
    let res_jwt = quick_onboard_user(&env, "resident_des5@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let rejected = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    );
    assert_eq!(
        rejected,
        Err(400),
        "a signer who no longer holds the required designation must not be attachable"
    );
}

async fn requirement_status(env: &TestEnv, requirement_id: i64) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT status::TEXT FROM document_signature_requirements WHERE requirement_id = $1",
    )
    .bind(requirement_id as i32)
    .fetch_one(&env.db_pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn test_revoking_designation_after_attach_blocks_signing() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (jwt1, signer1, uid1, designation1) = make_signer(&env, &admin_jwt, "des6a", "234");

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des6@sigtest.test", "SG", "235");
    let res_jwt = quick_onboard_user(&env, "resident_des6@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .expect("attach should succeed while signer1 still holds the designation");
    let req1 = requirement_id_for_signer(&requirements, signer1);

    // Designation revoked AFTER attachment but BEFORE signing.
    quick_revoke_designation(&env, &admin_jwt, uid1, &designation1);

    let result = sign_requirement(&url, &auth(&jwt1), req1);
    assert!(
        result.is_err(),
        "signing must be rejected once the signer no longer holds the field's required designation"
    );
    assert_eq!(
        requirement_status(&env, req1).await,
        "PENDING",
        "the requirement must remain PENDING — nothing should have been wrongly completed"
    );

    assert!(
        !is_step_completed(&env, &pid, step_id).await,
        "the step must stay blocked since the only attached signer could not validly sign"
    );
}

#[tokio::test]
async fn test_revoking_designation_mid_sequence_blocks_later_signer() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (jwt1, signer1, _uid1, designation1) = make_signer(&env, &admin_jwt, "des7a", "236");
    let (jwt2, signer2, uid2, designation2) = make_signer(&env, &admin_jwt, "des7b", "237");

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_with_required_designations(&[&designation1, &designation2]),
    );

    quick_create_user(&env, &admin_jwt, "resident_des7@sigtest.test", "SG", "238");
    let res_jwt = quick_onboard_user(&env, "resident_des7@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1, signer2],
    )
    .expect("both signers hold the required designations at attach time");
    let req1 = requirement_id_for_signer(&requirements, signer1);
    let req2 = requirement_id_for_signer(&requirements, signer2);

    // Revoke signer2's designation while it's still signer1's turn.
    quick_revoke_designation(&env, &admin_jwt, uid2, &designation2);

    sign_requirement(&url, &auth(&jwt1), req1)
        .expect("first signer is unaffected and should succeed");

    let result = sign_requirement(&url, &auth(&jwt2), req2);
    assert!(
        result.is_err(),
        "second signer must be rejected once their required designation was revoked, even though it's now their turn"
    );
    assert_eq!(requirement_status(&env, req2).await, "PENDING");
}

/// A signer doesn't hold `process.reviewer`/`process.view` and doesn't own the
/// process, but they DO have a legitimate, narrow reason to view it: it contains the
/// document their signature concerns. That should be allowed read-only, and only once
/// they're actually attached — not before.
#[tokio::test]
async fn test_signer_can_view_process_they_are_attached_to() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(&env, &admin_jwt, "resident_view1@sigtest.test", "SG", "245");
    let res_jwt = quick_onboard_user(&env, "resident_view1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "sigview1", "246");

    let denied = get_process_state(&url, &auth(&jwt1), &pid);
    assert_eq!(
        denied,
        Err(403),
        "an unattached signer must not be able to view someone else's process"
    );

    add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .unwrap();

    let allowed = get_process_state(&url, &auth(&jwt1), &pid);
    assert!(
        allowed.is_ok(),
        "an attached signer should be able to view the process their signature concerns"
    );
}

/// A process reviewer needs to pick the right person, not browse their private
/// contact info — `/admin/signers/list` must never leak `phone`/`email`, only the
/// signer themselves can see those (via `/profile/signer-info`).
#[tokio::test]
async fn test_signer_directory_list_excludes_contact_info() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (_jwt1, _signer1, ..) = make_signer(&env, &admin_jwt, "sigprivacy", "247");

    let signers: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/signers/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "include_inactive": true }))
        .expect("signers/list should succeed for process.reviewer")
        .into_json()
        .unwrap();
    assert_eq!(signers.len(), 1);
    let entry = &signers[0];
    assert!(entry.get("name").is_some(), "name should be present");
    assert!(
        entry.get("user_email").is_some(),
        "login email should be present"
    );
    assert!(
        entry.get("phone").is_none(),
        "phone must not be exposed to the reviewer-facing directory listing"
    );
    assert!(
        entry.get("email").is_none(),
        "the signer's registered contact email must not be exposed to the directory listing"
    );
}

// ==========================================================================
// `requires_signature` — a template field can require a signature independently of
// `required_signs_from_designations` (which only scopes/orders *who* is eligible, not
// *whether* a signature is required at all). The two flags are orthogonal: a field can
// require a signature from ANY registered signer (empty designation list = no
// restriction), or require one from a specific designation, or not require one at all.
// ==========================================================================

/// A document field with `requires_signature: true` and NO designation restriction: the
/// step must stay blocked until a signer is attached AND signs, even though nothing
/// constrains WHO can be attached. This is the exact "leave the designation list empty"
/// scenario — `requires_signature` must not silently become false just because there's
/// no designation restriction.
#[tokio::test]
async fn test_requires_signature_blocks_step_with_no_designation_restriction() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) =
        save_template_draft(&url, &admin_jwt, template_draft_requires_signature(&[]));

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_reqsig1@sigtest.test",
        "SG",
        "250",
    );
    let res_jwt = quick_onboard_user(&env, "resident_reqsig1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");

    // Document is uploaded, but no signer has been attached yet.
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);
    assert!(
        !is_step_completed(&env, &pid, step_id).await,
        "requires_signature must block completion even with no designation restriction, until a signer is attached"
    );

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "reqsig1a", "251");
    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .expect("attaching an unrestricted signer should succeed — any registered signer is eligible");
    let req1 = requirement_id_for_signer(&requirements, signer1);

    assert!(
        !is_step_completed(&env, &pid, step_id).await,
        "step must stay blocked while the attached signer hasn't signed yet"
    );

    sign_requirement(&url, &auth(&jwt1), req1).expect("signing should succeed");

    assert!(
        is_step_completed(&env, &pid, step_id).await,
        "step must complete once the only attached signer has signed"
    );
}

/// Control case: a field WITHOUT `requires_signature` completes normally the moment its
/// document is submitted, with no signer ever attached — confirms `requires_signature`
/// (not `required_signs_from_designations`, and not document presence alone) is what
/// gates completion.
#[tokio::test]
async fn test_field_without_requires_signature_completes_without_any_signer() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let no_signature_draft = serde_json::json!({
        "template_id": null,
        "name": "Sig Test Template No Signature",
        "substeps": [
            {
                "name": "Doc Stage",
                "fields": [
                    { "name": "NOC Document", "type": "document", "filled_by": "admin" }
                ]
            }
        ]
    });
    let (template_id, step_id, field_id) =
        save_template_draft(&url, &admin_jwt, no_signature_draft);

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_reqsig2@sigtest.test",
        "SG",
        "252",
    );
    let res_jwt = quick_onboard_user(&env, "resident_reqsig2@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");

    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    assert!(
        is_step_completed(&env, &pid, step_id).await,
        "a field with no requires_signature must complete on document submission alone, no signer needed"
    );
}

/// The two flags combined: `requires_signature: true` AND a designation restriction.
/// Both must hold — the step stays blocked until a signer who actually satisfies the
/// designation requirement has been attached and has signed.
#[tokio::test]
async fn test_requires_signature_combined_with_designation_restriction() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let (jwt1, signer1, _uid1, designation1) = make_signer(&env, &admin_jwt, "reqsig3a", "253");
    let (_jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "reqsig3b", "254");

    let (template_id, step_id, field_id) = save_template_draft(
        &url,
        &admin_jwt,
        template_draft_requires_signature(&[&designation1]),
    );

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_reqsig3@sigtest.test",
        "SG",
        "255",
    );
    let res_jwt = quick_onboard_user(&env, "resident_reqsig3@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    assert!(
        !is_step_completed(&env, &pid, step_id).await,
        "requires_signature must block completion until a qualifying signer is attached"
    );

    let rejected = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer2],
    );
    assert_eq!(
        rejected,
        Err(400),
        "a signer without the required designation must still be rejected on attach"
    );
    assert!(
        !is_step_completed(&env, &pid, step_id).await,
        "a rejected attach attempt must not have changed completion state"
    );

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .expect("the qualifying signer should be accepted");
    let req1 = requirement_id_for_signer(&requirements, signer1);

    assert!(!is_step_completed(&env, &pid, step_id).await);

    sign_requirement(&url, &auth(&jwt1), req1).expect("signing should succeed");

    assert!(
        is_step_completed(&env, &pid, step_id).await,
        "step must complete once the qualifying signer has signed"
    );
}

/// A 2-signer chain produces an intermediate signed-replacement document after the
/// first signer, but the field must stay pointed at the ORIGINAL document until every
/// signer has signed — only the LAST signer's replacement ever gets attached. This is
/// the scenario behind a real bug report: the frontend was showing both the
/// pre-signature original and the final signed copy as two separate "Document:" rows
/// because it merged in full field history instead of just the current value; the
/// underlying guarantee this test protects is that there's only ever one CURRENT
/// document once the chain completes, and it's the final signer's copy, not an
/// intermediate one.
#[tokio::test]
async fn test_field_only_repoints_to_final_document_after_full_signer_chain() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_chain1@sigtest.test",
        "SG",
        "260",
    );
    let res_jwt = quick_onboard_user(&env, "resident_chain1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let original_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &original_doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "chain1a", "261");
    let (jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "chain1b", "262");

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &original_doc_id,
        &[signer1, signer2],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);
    let req2 = requirement_id_for_signer(&requirements, signer2);

    let (_confirm1, first_signed_doc_id) = sign_requirement_with_new_doc(&url, &auth(&jwt1), req1)
        .expect("first signer should succeed");

    let state_after_first = get_process_state(&url, &admin_auth, &pid).unwrap();
    assert_eq!(
        field_document_id(&state_after_first, field_id),
        Some(original_doc_id.clone()),
        "field must still point at the original document while a later signer is outstanding — \
         the first signer's replacement must not be attached yet"
    );

    let (_confirm2, second_signed_doc_id) = sign_requirement_with_new_doc(&url, &auth(&jwt2), req2)
        .expect("second signer should succeed");
    assert_ne!(
        first_signed_doc_id, second_signed_doc_id,
        "each signer in the chain must produce its own distinct replacement document"
    );

    let state_after_second = get_process_state(&url, &admin_auth, &pid).unwrap();
    assert_eq!(
        field_document_id(&state_after_second, field_id),
        Some(second_signed_doc_id),
        "once every signer has signed, the field must point at the LAST signer's document — \
         not the original and not the first signer's intermediate copy"
    );
}

/// The second signer in a chain must see and sign the FIRST signer's output, not a
/// blind copy of the pristine original — otherwise the "chain" never actually
/// accumulates signatures, it's just N independent signings of the same input.
#[tokio::test]
async fn test_second_signer_input_document_is_first_signers_output() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_chain2@sigtest.test",
        "SG",
        "272",
    );
    let res_jwt = quick_onboard_user(&env, "resident_chain2@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let original_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &original_doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "chain2a", "273");
    let (jwt2, signer2, ..) = make_signer(&env, &admin_jwt, "chain2b", "274");

    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &original_doc_id,
        &[signer1, signer2],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);
    let req2 = requirement_id_for_signer(&requirements, signer2);

    let (_confirm1, first_signed_doc_id) = sign_requirement_with_new_doc(&url, &auth(&jwt1), req1)
        .expect("first signer should succeed");

    let doc: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/document-download-url",
        url
    ))
    .set("Authorization", &auth(&jwt2))
    .send_json(serde_json::json!({ "requirement_id": req2 }))
    .expect("second signer should be able to view the document to sign")
    .into_json()
    .unwrap();
    assert_eq!(
        doc["document"]["document_id"].as_str(),
        Some(first_signed_doc_id.as_str()),
        "the second signer must be shown the FIRST signer's signed copy, not the original"
    );

    let signer1_denied = ureq::post(&format!(
        "{}/api/v1/admin/documents/signatures/document-download-url",
        url
    ))
    .set("Authorization", &auth(&jwt1))
    .send_json(serde_json::json!({ "requirement_id": req2 }));
    assert!(
        signer1_denied.is_err(),
        "a signer must not be able to view a document for a requirement that isn't theirs"
    );
}

/// If the field gets resaved with a brand-new document while a signature requirement
/// is still pending on the OLD document (e.g. an admin re-uploads a corrected file
/// before the signer got to it), that requirement is now stale. The signer can still
/// complete it (there's no cancellation), but doing so must NOT repoint the field back
/// at a signed copy of the superseded document — `attach_replacement_and_recompute_gating`
/// checks `is_current_field_value` before attaching for exactly this reason.
#[tokio::test]
async fn test_stale_signature_requirement_does_not_repoint_field_after_resave() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) = save_template(&url, &admin_jwt);

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_stale1@sigtest.test",
        "SG",
        "263",
    );
    let res_jwt = quick_onboard_user(&env, "resident_stale1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let old_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc-v1.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &old_doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "stale1a", "264");
    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &old_doc_id,
        &[signer1],
    )
    .unwrap();
    let req1 = requirement_id_for_signer(&requirements, signer1);

    // Resave the field with a new document before the signer gets to it — this
    // supersedes the old document without going through any signature flow at all.
    let new_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc-v2.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &new_doc_id);

    let (confirm, stale_signed_doc_id) = sign_requirement_with_new_doc(&url, &auth(&jwt1), req1)
        .expect("a stale requirement can still be completed — there's no cancellation");
    assert_eq!(confirm["status"], "COMPLETED");

    let state = get_process_state(&url, &admin_auth, &pid).unwrap();
    assert_eq!(
        field_document_id(&state, field_id),
        Some(new_doc_id),
        "the field must stay pointed at the newer document that superseded the one \
         the stale requirement was attached to"
    );
    assert_ne!(
        field_document_id(&state, field_id),
        Some(stale_signed_doc_id),
        "a stale requirement's signed replacement must never be attached to the field"
    );
}

/// `signature_status` must still resolve after the field repoints to the signed copy.
#[tokio::test]
async fn test_signature_status_still_resolves_after_field_repoints_to_signed_copy() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) =
        save_template_draft(&url, &admin_jwt, template_draft_requires_signature(&[]));

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_repoint1@sigtest.test",
        "SG",
        "270",
    );
    let res_jwt = quick_onboard_user(&env, "resident_repoint1@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);
    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "noc.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &doc_id);

    let (jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "repoint1a", "271");
    let requirements = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &doc_id,
        &[signer1],
    )
    .expect("attach should succeed");
    let req1 = requirement_id_for_signer(&requirements, signer1);
    let (_confirm, signed_doc_id) =
        sign_requirement_with_new_doc(&url, &auth(&jwt1), req1).expect("signing should succeed");

    let state = get_process_state(&url, &admin_auth, &pid).unwrap();
    assert_eq!(
        field_document_id(&state, field_id),
        Some(signed_doc_id),
        "field must be repointed at the signed copy once the only signer has signed"
    );

    let statuses = field_signature_statuses(&state, field_id);
    assert_eq!(
        statuses.len(),
        1,
        "signature_status must still show the (now completed) requirement after repoint, not go empty"
    );
    assert_eq!(statuses[0]["status"], "COMPLETED");

    // Also confirm the resident-facing view agrees it's actually signed, not stuck
    // looking "awaiting setup" (empty signature_status + requires_signature would
    // otherwise read as "no signer ever attached").
    let resident_state = get_process_state(&url, &auth(&res_jwt), &pid).unwrap();
    let resident_statuses = field_signature_statuses(&resident_state, field_id);
    assert_eq!(resident_statuses.len(), 1);
    assert_eq!(resident_statuses[0]["status"], "COMPLETED");
}

/// `add-signers` must reject a `document_id` that isn't the field's current document —
/// otherwise a caller could route signers (and, for provider="signyu", a real external
/// e-sign request) to a stale/unrelated document while claiming a valid
/// (process_id, template_step_id, template_field_id) triple.
#[tokio::test]
async fn test_add_signers_rejects_stale_document_id() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let (template_id, step_id, field_id) =
        save_template_draft(&url, &admin_jwt, template_draft_requires_signature(&[]));

    quick_create_user(&env, &admin_jwt, "resident_stale@sigtest.test", "SG", "260");
    let res_jwt = quick_onboard_user(&env, "resident_stale@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);

    let stale_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "stale.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &stale_doc_id);

    let current_doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "current.pdf");
    save_step(&url, &admin_auth, &pid, step_id, field_id, &current_doc_id);

    let (_jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "stale1", "261");

    let stale_result = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &stale_doc_id,
        &[signer1],
    );
    assert_eq!(
        stale_result,
        Err(400),
        "attaching signers to a superseded document_id must be rejected"
    );

    let current_result = add_signers(
        &url,
        &admin_auth,
        &pid,
        step_id,
        field_id,
        &current_doc_id,
        &[signer1],
    );
    assert!(
        current_result.is_ok(),
        "attaching signers to the field's actual current document must still succeed"
    );
}

/// `add-signers` must reject a `template_field_id` that doesn't actually belong to the
/// claimed `template_step_id`, even when both IDs are independently valid for the
/// process — otherwise a reviewer authorized on one step could attach signers (and
/// route a real document to SignYu) under a different step's identity.
#[tokio::test]
async fn test_add_signers_rejects_field_not_belonging_to_step() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let draft = serde_json::json!({
        "template_id": null,
        "name": "Sig Test Two Stage Template",
        "substeps": [
            {
                "name": "Stage One",
                "fields": [
                    { "name": "Doc One", "type": "document", "filled_by": "admin", "requires_signature": true }
                ]
            },
            {
                "name": "Stage Two",
                "fields": [
                    { "name": "Doc Two", "type": "document", "filled_by": "admin", "requires_signature": true }
                ]
            }
        ]
    });
    let save_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
            .set("Authorization", &admin_auth)
            .send_json(draft)
            .expect("template save failed")
            .into_json()
            .unwrap();
    let version_id = save_resp["version_id"].as_i64().unwrap() as i32;

    let templates: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/process-templates/list", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    let template_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(version_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;
    ureq::post(&format!("{}/api/v1/admin/process-templates/activate", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "template_id": template_id, "version_id": version_id }))
        .expect("activate failed");

    let tree: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/get-version",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "version_id": version_id }))
    .unwrap()
    .into_json()
    .unwrap();
    let stage1_step_id = tree["substeps"][0]["step_id"].as_i64().unwrap() as i32;
    let field1_id = tree["substeps"][0]["fields"][0]["field_id"]
        .as_i64()
        .unwrap() as i32;
    let _stage2_step_id = tree["substeps"][1]["step_id"].as_i64().unwrap() as i32;
    let field2_id = tree["substeps"][1]["fields"][0]["field_id"]
        .as_i64()
        .unwrap() as i32;

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_mismatch@sigtest.test",
        "SG",
        "262",
    );
    let res_jwt = quick_onboard_user(&env, "resident_mismatch@sigtest.test", "password123");
    let pid = start_process(&url, &res_jwt, template_id);

    let doc_id = upload_doc_as_admin(&url, &admin_auth, &pid, "mismatch.pdf");
    save_step(&url, &admin_auth, &pid, stage1_step_id, field1_id, &doc_id);

    let (_jwt1, signer1, ..) = make_signer(&env, &admin_jwt, "mismatch1", "263");

    // stage1_step_id is genuinely Open, but field2_id belongs to stage2 — must be
    // rejected even though both IDs are independently valid for this process.
    let mismatched_result = add_signers(
        &url,
        &admin_auth,
        &pid,
        stage1_step_id,
        field2_id,
        &doc_id,
        &[signer1],
    );
    assert_eq!(
        mismatched_result, Err(400),
        "attaching signers with a field_id that doesn't belong to the claimed step_id must be rejected"
    );
}
