//! End-to-end coverage of who may attach which document to a process step via
//! `save-step`, with callers holding only a designation (no document permission, no
//! `process.reviewer`), only `process.reviewer`, or just a resident's own standing.

use quality::{
    quick_assign_designation, quick_create_designation, quick_create_user, quick_onboard_user,
    quick_setup_admin, quick_toggle_designation_permission, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn template_draft(name: &str, filled_by: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": name,
        "description": null,
        "is_optional": false,
        "substeps": [{
            "name": "Upload Stage",
            "filled_by": filled_by,
            "fields": [{ "name": "Doc", "type": "document" }]
        }]
    })
}

fn create_and_activate_template(url: &str, admin_jwt: &str, draft: serde_json::Value) -> i32 {
    let saved: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
            .set("Authorization", &auth(admin_jwt))
            .send_json(draft)
            .expect("save template")
            .into_json()
            .unwrap();
    let version_id = saved["version_id"].as_i64().unwrap();

    let templates: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/process-templates/list", url))
            .set("Authorization", &auth(admin_jwt))
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
                .any(|v| v["version_id"].as_i64() == Some(version_id))
        })
        .and_then(|t| t["template_id"].as_i64())
        .expect("template_id for saved version") as i32;

    ureq::post(&format!("{}/api/v1/admin/process-templates/activate", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({ "template_id": template_id, "version_id": version_id }))
        .expect("activate template");
    template_id
}

fn mixed_template_draft() -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Resident Then Secretary",
        "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Resident Stage",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Doc", "type": "document" }]
            },
            {
                "name": "Secretary Stage",
                "filled_by": { "kind": "designation", "code": "secretary" },
                "fields": [{ "name": "Doc", "type": "document" }]
            }
        ]
    })
}

struct StartedProcess {
    process_id: String,
    step_id: i64,
    field_id: i64,
}

fn start_process(url: &str, jwt: &str, template_id: i32) -> StartedProcess {
    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .expect("start process")
        .into_json()
        .unwrap();
    let step = &state["steps"][0];
    StartedProcess {
        process_id: state["process_id"].as_str().unwrap().to_string(),
        step_id: step["template_step_id"].as_i64().unwrap(),
        field_id: step["fields"][0]["template_field_id"].as_i64().unwrap(),
    }
}

fn put_bytes(upload_url: &str) {
    ureq::put(upload_url)
        .set("Content-Type", "application/pdf")
        .send_bytes(b"attach test pdf bytes")
        .expect("S3 PUT upload failed");
}

fn status_of<T>(resp: Result<T, ureq::Error>) -> Result<T, u16> {
    match resp {
        Ok(v) => Ok(v),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

/// Starts a reviewer upload on `process_id`; returns the new document id (unconfirmed).
fn reviewer_start_upload(url: &str, jwt: &str, process_id: &str) -> Result<String, u16> {
    let resp = status_of(
        ureq::post(&format!("{}/api/v1/admin/process/upload-url", url))
            .set("Authorization", &auth(jwt))
            .send_json(serde_json::json!({
                "process_id": process_id,
                "title": "Reviewer Upload",
                "filename": "reviewer_upload.pdf",
                "content_type": "application/pdf"
            })),
    )?;
    let body: serde_json::Value = resp.into_json().unwrap();
    put_bytes(body["upload_url"].as_str().unwrap());
    Ok(body["document_id"].as_str().unwrap().to_string())
}

fn reviewer_upload_confirmed(url: &str, jwt: &str, process_id: &str) -> String {
    let doc = reviewer_start_upload(url, jwt, process_id).expect("reviewer upload-url");
    ureq::post(&format!("{}/api/v1/admin/process/confirm-upload", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "process_id": process_id, "document_id": doc }))
        .expect("reviewer confirm-upload");
    doc
}

fn resident_upload_confirmed(url: &str, jwt: &str, process_id: &str) -> String {
    let body: serde_json::Value = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "title": "Resident Upload",
            "filename": "resident_upload.pdf",
            "content_type": "application/pdf"
        }))
        .expect("resident upload-url")
        .into_json()
        .unwrap();
    let doc = body["document_id"].as_str().unwrap().to_string();
    put_bytes(body["upload_url"].as_str().unwrap());
    ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "process_id": process_id, "document_id": doc }))
        .expect("resident confirm-upload");
    doc
}

fn attach(url: &str, jwt: &str, p: &StartedProcess, document_id: &str) -> Result<(), u16> {
    status_of(
        ureq::post(&format!("{}/api/v1/process/save-step", url))
            .set("Authorization", &auth(jwt))
            .send_json(serde_json::json!({
                "process_id": p.process_id,
                "template_step_id": p.step_id,
                "fields": [{
                    "template_field_id": p.field_id,
                    "text_value": null,
                    "document_id": document_id
                }]
            })),
    )
    .map(|_| ())
}

fn download(url: &str, jwt: &str, process_id: &str, document_id: &str) -> Result<(), u16> {
    status_of(
        ureq::post(&format!("{}/api/v1/process/download-url", url))
            .set("Authorization", &auth(jwt))
            .send_json(serde_json::json!({
                "process_id": process_id,
                "document_id": document_id
            })),
    )
    .map(|_| ())
}

/// Two flats (A-101, A-102), each with a resident; a `secretary` designation holder with
/// no permissions at all; a `treasurer` holder owning no stage in any template here; and
/// a `process.reviewer`-only user.
struct Fixture {
    env: TestEnv,
    url: String,
    res1_jwt: String,
    res2_jwt: String,
    secretary_jwt: String,
    treasurer_jwt: String,
    reviewer_jwt: String,
    /// Secretary-owned stage on flat A-101.
    desig_p1: StartedProcess,
    /// Resident-owned stage on flat A-101.
    user_p1: StartedProcess,
    /// Open resident-owned first stage, then a secretary-owned stage, on flat A-101.
    mixed_p1: StartedProcess,
    /// Secretary-owned stage on flat A-102.
    desig_p2: StartedProcess,
    /// Resident-owned stage on flat A-102.
    user_p2: StartedProcess,
}

async fn fixture() -> Fixture {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");
    quick_create_designation(&env, &admin_jwt, "treasurer", "Treasurer");
    quick_create_designation(&env, &admin_jwt, "reviewer_only", "Reviewer Only");
    quick_toggle_designation_permission(
        &env,
        &admin_jwt,
        "reviewer_only",
        "process.reviewer",
        true,
    );

    let desig_tpl = create_and_activate_template(
        &url,
        &admin_jwt,
        template_draft(
            "Secretary Upload",
            serde_json::json!({ "kind": "designation", "code": "secretary" }),
        ),
    );
    let user_tpl = create_and_activate_template(
        &url,
        &admin_jwt,
        template_draft("Resident Upload", serde_json::json!({ "kind": "user" })),
    );

    let mixed_tpl = create_and_activate_template(&url, &admin_jwt, mixed_template_draft());

    quick_create_user(&env, &admin_jwt, "attach_res1@dtest.test", "A", "101");
    let res1_jwt = quick_onboard_user(&env, "attach_res1@dtest.test", "password123");
    quick_create_user(&env, &admin_jwt, "attach_res2@dtest.test", "A", "102");
    let res2_jwt = quick_onboard_user(&env, "attach_res2@dtest.test", "password123");

    let secretary_id = quick_create_user(&env, &admin_jwt, "attach_sec@dtest.test", "B", "201");
    let secretary_jwt = quick_onboard_user(&env, "attach_sec@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, secretary_id, "secretary");

    let treasurer_id = quick_create_user(&env, &admin_jwt, "attach_tre@dtest.test", "B", "202");
    let treasurer_jwt = quick_onboard_user(&env, "attach_tre@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, treasurer_id, "treasurer");

    let reviewer_id = quick_create_user(&env, &admin_jwt, "attach_rev@dtest.test", "B", "203");
    let reviewer_jwt = quick_onboard_user(&env, "attach_rev@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, reviewer_id, "reviewer_only");

    let desig_p1 = start_process(&url, &res1_jwt, desig_tpl);
    let user_p1 = start_process(&url, &res1_jwt, user_tpl);
    let mixed_p1 = start_process(&url, &res1_jwt, mixed_tpl);
    let desig_p2 = start_process(&url, &res2_jwt, desig_tpl);
    let user_p2 = start_process(&url, &res2_jwt, user_tpl);

    Fixture {
        env,
        url,
        res1_jwt,
        res2_jwt,
        secretary_jwt,
        treasurer_jwt,
        reviewer_jwt,
        desig_p1,
        user_p1,
        mixed_p1,
        desig_p2,
        user_p2,
    }
}

#[tokio::test]
async fn test_designation_holder_attaches_own_upload_and_can_download_it() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);

    attach(&f.url, &f.secretary_jwt, &f.desig_p1, &doc)
        .expect("designation holder with no document permission must attach their own upload");

    download(&f.url, &f.secretary_jwt, &f.desig_p1.process_id, &doc)
        .expect("the holder can download the document they attached");
    download(&f.url, &f.res1_jwt, &f.desig_p1.process_id, &doc)
        .expect("the flat's resident can download it too");

    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", f.url))
        .set("Authorization", &auth(&f.secretary_jwt))
        .send_json(serde_json::json!({ "process_id": f.desig_p1.process_id }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        state["steps"][0]["fields"][0]["value"]["document_id"], doc,
        "the attached document id must be stored on the field"
    );
}

#[tokio::test]
async fn test_designation_holder_attaches_residents_document_from_same_flat() {
    let f = fixture().await;
    let doc = resident_upload_confirmed(&f.url, &f.res1_jwt, &f.user_p1.process_id);

    attach(&f.url, &f.secretary_jwt, &f.desig_p1, &doc)
        .expect("a stage owner may attach any document on the process's own flat");
}

#[tokio::test]
async fn test_designation_holder_cannot_attach_on_a_stage_they_do_not_own() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.mixed_p1.process_id);

    assert_eq!(
        attach(&f.url, &f.secretary_jwt, &f.mixed_p1, &doc),
        Err(400),
        "the resident's stage in a process the secretary can see is still not theirs to fill"
    );
}

#[tokio::test]
async fn test_designation_holder_has_no_access_to_a_process_with_none_of_their_stages() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);

    assert_eq!(
        attach(&f.url, &f.secretary_jwt, &f.user_p1, &doc),
        Err(403),
        "a process with no secretary-owned stage is closed to the secretary entirely"
    );
    assert_eq!(
        reviewer_start_upload(&f.url, &f.secretary_jwt, &f.user_p1.process_id),
        Err(403)
    );
}

#[tokio::test]
async fn test_designation_holder_without_a_stage_in_the_process_has_no_standing() {
    let f = fixture().await;

    assert_eq!(
        reviewer_start_upload(&f.url, &f.treasurer_jwt, &f.desig_p1.process_id),
        Err(403),
        "a designation owning no stage here must not start an upload"
    );

    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);
    assert_eq!(
        attach(&f.url, &f.treasurer_jwt, &f.desig_p1, &doc),
        Err(403),
        "nor attach to a stage owned by another designation"
    );
}

#[tokio::test]
async fn test_cannot_attach_document_from_another_flat() {
    let f = fixture().await;
    let doc_flat_101 = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);

    assert_eq!(
        attach(&f.url, &f.secretary_jwt, &f.desig_p2, &doc_flat_101),
        Err(400),
        "a flat A-101 document must not attach to a flat A-102 process, even for a stage owner"
    );
}

#[tokio::test]
async fn test_resident_cannot_attach_document_from_another_flat() {
    let f = fixture().await;
    let doc_flat_101 = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);

    assert_eq!(
        attach(&f.url, &f.res2_jwt, &f.user_p2, &doc_flat_101),
        Err(400),
        "a resident must not attach another flat's document to their own stage"
    );
}

#[tokio::test]
async fn test_resident_attaches_document_a_reviewer_uploaded_to_their_flat() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);

    attach(&f.url, &f.res1_jwt, &f.user_p1, &doc)
        .expect("the flat's owner can use a document staff uploaded to their flat");
}

#[tokio::test]
async fn test_unconfirmed_document_is_rejected_for_a_designation_holder() {
    let f = fixture().await;
    let doc = reviewer_start_upload(&f.url, &f.secretary_jwt, &f.desig_p1.process_id).unwrap();

    assert_eq!(
        attach(&f.url, &f.secretary_jwt, &f.desig_p1, &doc),
        Err(400)
    );
}

#[tokio::test]
async fn test_deleted_document_is_rejected_for_a_designation_holder() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);
    sqlx::query("UPDATE documents SET is_deleted = TRUE WHERE document_id = $1")
        .bind(uuid::Uuid::parse_str(&doc).unwrap())
        .execute(&f.env.db_pool)
        .await
        .unwrap();

    assert_eq!(
        attach(&f.url, &f.secretary_jwt, &f.desig_p1, &doc),
        Err(400)
    );
}

#[tokio::test]
async fn test_non_resident_specific_document_is_rejected_for_a_designation_holder() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);
    let other_category: String = sqlx::query_scalar(
        "SELECT code FROM document_categories WHERE code <> 'RESIDENT_SPECIFIC' LIMIT 1",
    )
    .fetch_one(&f.env.db_pool)
    .await
    .expect("another document category");
    sqlx::query("UPDATE documents SET category = $1 WHERE document_id = $2")
        .bind(&other_category)
        .bind(uuid::Uuid::parse_str(&doc).unwrap())
        .execute(&f.env.db_pool)
        .await
        .unwrap();

    assert_eq!(
        attach(&f.url, &f.secretary_jwt, &f.desig_p1, &doc),
        Err(400),
        "only flat-scoped Resident Specific documents may sit in a process field"
    );
}

#[tokio::test]
async fn test_nonexistent_document_id_is_rejected() {
    let f = fixture().await;

    assert_eq!(
        attach(
            &f.url,
            &f.secretary_jwt,
            &f.desig_p1,
            &uuid::Uuid::new_v4().to_string()
        ),
        Err(400)
    );
}

#[tokio::test]
async fn test_process_reviewer_without_designation_attaches_on_resident_stage() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.reviewer_jwt, &f.user_p1.process_id);

    attach(&f.url, &f.reviewer_jwt, &f.user_p1, &doc)
        .expect("a global reviewer fills the resident's stage with their own upload");
    assert_eq!(
        attach(&f.url, &f.reviewer_jwt, &f.user_p2, &doc),
        Err(400),
        "but never with a document from a different flat"
    );
    assert_eq!(
        attach(&f.url, &f.reviewer_jwt, &f.desig_p1, &doc),
        Err(400),
        "and a global reviewer still can't fill a stage owned by a designation they don't hold"
    );
}

#[tokio::test]
async fn test_unrelated_user_cannot_attach_or_upload_on_someone_elses_process() {
    let f = fixture().await;
    let doc = reviewer_upload_confirmed(&f.url, &f.secretary_jwt, &f.desig_p1.process_id);

    assert_eq!(
        attach(&f.url, &f.res2_jwt, &f.user_p1, &doc),
        Err(403),
        "another flat's resident has no access to this process at all"
    );
    assert_eq!(
        reviewer_start_upload(&f.url, &f.res2_jwt, &f.user_p1.process_id),
        Err(403)
    );
}
