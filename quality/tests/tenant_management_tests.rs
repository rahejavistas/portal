//! Tenant mapping (`tenant_ownerships`) and `StepOwner::Tenant` authorization —
//! through the real HTTP API and a real database.

use quality::{
    quick_assign_designation, quick_create_designation, quick_create_document, quick_create_user,
    quick_onboard_user, quick_setup_admin, quick_toggle_designation_permission,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn tenant_draft() -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Tenant Step Test Process",
        "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Tenant Acknowledgement",
                "filled_by": { "kind": "tenant" },
                "fields": [
                    { "name": "Note", "type": "text" }
                ]
            }
        ]
    })
}

fn create_and_activate_template(url: &str, admin_jwt: &str, draft: &serde_json::Value) -> i32 {
    let save_resp: serde_json::Value =
        match ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
            .set("Authorization", &auth(admin_jwt))
            .send_json(draft.clone())
        {
            Ok(r) => r.into_json().unwrap(),
            Err(ureq::Error::Status(code, r)) => {
                panic!(
                    "save template failed {}: {}",
                    code,
                    r.into_string().unwrap()
                )
            }
            Err(e) => panic!("transport error: {:?}", e),
        };
    let version_id = save_resp["version_id"].as_i64().unwrap() as i32;

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
                .any(|v| v["version_id"].as_i64() == Some(version_id as i64))
        })
        .and_then(|t| t["template_id"].as_i64())
        .expect("template_id for saved version") as i32;

    ureq::post(&format!("{}/api/v1/admin/process-templates/activate", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({ "template_id": template_id, "version_id": version_id }))
        .expect("activate template");

    template_id
}

fn start_process(url: &str, jwt: &str, template_id: i32) -> serde_json::Value {
    ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .expect("start process")
        .into_json()
        .unwrap()
}

fn save_step(
    url: &str,
    jwt: &str,
    process_id: &str,
    template_step_id: i64,
    template_field_id: i64,
) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": template_step_id,
            "fields": [{
                "template_field_id": template_field_id,
                "text_value": "Acknowledged.",
                "document_id": null
            }]
        }));
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn field_and_step_ids(state: &serde_json::Value) -> (i64, i64) {
    let step = &state["steps"][0];
    let step_id = step["template_step_id"].as_i64().unwrap();
    let field_id = step["fields"][0]["template_field_id"].as_i64().unwrap();
    (step_id, field_id)
}

fn add_tenant(
    url: &str,
    owner_jwt: &str,
    ownership_id: i64,
    tenant_email: &str,
) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!("{}/api/v1/tenants/add", url))
        .set("Authorization", &auth(owner_jwt))
        .send_json(serde_json::json!({
            "ownership_id": ownership_id,
            "tenant_email": tenant_email,
            "expires_at": "2027-01-01",
            "rental_agreement_document_id": null
        }));
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn owned_ownership_id(url: &str, admin_jwt: &str, owner_jwt: &str) -> i64 {
    // Starting any process against the owner's active flat returns its ownership_id.
    let dummy_draft = serde_json::json!({
        "template_id": null, "name": "Ownership Probe", "description": null,
        "is_optional": false,
        "substeps": [{"name": "S", "fields": [{"name": "N", "type": "text"}]}]
    });
    let template_id = create_and_activate_template(url, admin_jwt, &dummy_draft);
    let state = start_process(url, owner_jwt, template_id);
    state["ownership_id"].as_i64().unwrap()
}

/// The owner adds a tenant, the tenant can fill the `Tenant`-owned step, but the owner
/// themselves — holding no tenant mapping — cannot.
#[tokio::test]
async fn test_tenant_can_act_on_own_flats_tenant_step_but_owner_cannot() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner1@dtest.test", "A", "501");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner1@dtest.test", "password123");

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let ownership_id = state["ownership_id"]
        .as_i64()
        .expect("process must have an ownership");
    let (step_id, field_id) = field_and_step_ids(&state);

    let add_resp = add_tenant(&url, &owner_jwt, ownership_id, "tenant1@dtest.test")
        .expect("owner must be able to add a tenant for their own flat");
    assert!(add_resp["tenant_user_id"].as_i64().is_some());

    let tenant_jwt = quick_onboard_user(&env, "tenant1@dtest.test", "password123");

    let owner_attempt = save_step(&url, &owner_jwt, &process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        owner_attempt, 400,
        "the flat's owner, holding no tenant mapping, must not be able to fill a Tenant-owned step"
    );

    let tenant_result = save_step(&url, &tenant_jwt, &process_id, step_id, field_id)
        .expect("the current tenant must be able to fill their own step");
    assert_eq!(tenant_result["status"], "success");
    assert_eq!(tenant_result["step"]["is_completed"], true);
}

/// A different flat's tenant has zero standing on this flat's `Tenant`-owned step —
/// the classic cross-resource IDOR substitution.
#[tokio::test]
async fn test_a_different_flats_tenant_cannot_act_on_this_flats_tenant_step() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner2@dtest.test", "A", "502");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner2@dtest.test", "password123");
    quick_create_user(&env, &admin_jwt, "tenant_owner2b@dtest.test", "B", "502");
    let other_owner_jwt = quick_onboard_user(&env, "tenant_owner2b@dtest.test", "password123");

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let ownership_id = state["ownership_id"].as_i64().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    add_tenant(&url, &owner_jwt, ownership_id, "tenant2a@dtest.test").unwrap();
    let other_state = start_process(&url, &other_owner_jwt, template_id);
    let other_ownership_id = other_state["ownership_id"].as_i64().unwrap();
    add_tenant(
        &url,
        &other_owner_jwt,
        other_ownership_id,
        "tenant2b@dtest.test",
    )
    .unwrap();

    let other_tenant_jwt = quick_onboard_user(&env, "tenant2b@dtest.test", "password123");

    let err = save_step(&url, &other_tenant_jwt, &process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        err, 403,
        "a different flat's tenant must have no standing on this flat's Tenant step"
    );
}

/// Adding a new tenant deactivates the previous one — at most one active tenant per
/// flat, and the old tenant immediately loses access.
#[tokio::test]
async fn test_adding_a_new_tenant_deactivates_the_previous_one() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner3@dtest.test", "A", "503");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner3@dtest.test", "password123");

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let ownership_id = state["ownership_id"].as_i64().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    add_tenant(&url, &owner_jwt, ownership_id, "old_tenant@dtest.test").unwrap();
    let old_tenant_jwt = quick_onboard_user(&env, "old_tenant@dtest.test", "password123");

    add_tenant(&url, &owner_jwt, ownership_id, "new_tenant@dtest.test")
        .expect("owner must be able to replace the current tenant");
    let new_tenant_jwt = quick_onboard_user(&env, "new_tenant@dtest.test", "password123");

    let old_err = save_step(&url, &old_tenant_jwt, &process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        old_err, 403,
        "the replaced tenant must lose access immediately"
    );

    let new_result = save_step(&url, &new_tenant_jwt, &process_id, step_id, field_id)
        .expect("the new tenant must be able to act");
    assert_eq!(new_result["status"], "success");
}

/// A resident who does not own the flat cannot add a tenant for it — ownership-scoping
/// on the write path itself, not just on process access.
#[tokio::test]
async fn test_non_owner_cannot_add_a_tenant_for_someone_elses_flat() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner4@dtest.test", "A", "504");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner4@dtest.test", "password123");
    quick_create_user(&env, &admin_jwt, "tenant_intruder4@dtest.test", "B", "504");
    let intruder_jwt = quick_onboard_user(&env, "tenant_intruder4@dtest.test", "password123");

    let ownership_id = owned_ownership_id(&url, &admin_jwt, &owner_jwt);

    let err = add_tenant(
        &url,
        &intruder_jwt,
        ownership_id,
        "intruder_tenant@dtest.test",
    )
    .unwrap_err();
    assert_eq!(
        err, 403,
        "a caller who does not own the flat must not be able to add a tenant for it"
    );
}

/// A tenant's process list is scoped to only processes whose template has a
/// `Tenant`-owned step — never the owner's unrelated processes, even on the same flat.
#[tokio::test]
async fn test_tenant_process_list_excludes_processes_without_a_tenant_step() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner5@dtest.test", "A", "505");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner5@dtest.test", "password123");

    let tenant_template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let tenant_state = start_process(&url, &owner_jwt, tenant_template_id);
    let tenant_process_id = tenant_state["process_id"].as_str().unwrap().to_string();
    let ownership_id = tenant_state["ownership_id"].as_i64().unwrap();

    let owner_only_draft = serde_json::json!({
        "template_id": null, "name": "Owner Only Process", "description": null,
        "is_optional": false,
        "substeps": [{"name": "S", "fields": [{"name": "N", "type": "text"}]}]
    });
    let owner_only_template_id = create_and_activate_template(&url, &admin_jwt, &owner_only_draft);
    let owner_only_state = start_process(&url, &owner_jwt, owner_only_template_id);
    let owner_only_process_id = owner_only_state["process_id"].as_str().unwrap().to_string();

    add_tenant(&url, &owner_jwt, ownership_id, "listtenant5@dtest.test").unwrap();
    let tenant_jwt = quick_onboard_user(&env, "listtenant5@dtest.test", "password123");

    let listed: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({ "personal_only": true }))
        .unwrap()
        .into_json()
        .unwrap();
    let process_ids: Vec<&str> = listed["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["process_id"].as_str().unwrap())
        .collect();

    assert!(
        process_ids.contains(&tenant_process_id.as_str()),
        "the tenant must see the process that has a Tenant-owned step"
    );
    assert!(
        !process_ids.contains(&owner_only_process_id.as_str()),
        "the tenant must never see the owner's process that has no Tenant step"
    );
}

/// Flat documents (RESIDENT_SPECIFIC) are owner-scoped via `flat_ownerships`; a tenant
/// has no ownership row of their own, so they must never see them.
#[tokio::test]
async fn test_tenant_cannot_see_the_owners_flat_documents() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner6@dtest.test", "A", "506");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner6@dtest.test", "password123");

    let doc_id = quick_create_document(
        &env,
        &admin_jwt,
        "Owner's Flat Document",
        "RESIDENT_SPECIFIC",
        Some(("A", "506")),
    );

    let ownership_id = owned_ownership_id(&url, &admin_jwt, &owner_jwt);
    add_tenant(&url, &owner_jwt, ownership_id, "doctenant6@dtest.test").unwrap();
    let tenant_jwt = quick_onboard_user(&env, "doctenant6@dtest.test", "password123");

    let owner_docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/documents", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(owner_docs
        .iter()
        .any(|d| d["document"]["document_id"] == doc_id));

    let tenant_docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/documents", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        !tenant_docs
            .iter()
            .any(|d| d["document"]["document_id"] == doc_id),
        "a tenant must never see the owner's flat-scoped document"
    );

    let direct_get = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({}));
    assert!(
        direct_get.is_err(),
        "a tenant must also be refused direct access to the owner's document by ID"
    );
}

/// Even when the owner holds a real designation (and its permissions), none of that
/// translates to the tenant account — it is a wholly separate user with no
/// designations or permissions of its own.
#[tokio::test]
async fn test_owners_designation_never_translates_to_the_tenant() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");
    quick_toggle_designation_permission(&env, &admin_jwt, "secretary", "process.view", true);

    let owner_id = quick_create_user(&env, &admin_jwt, "tenant_owner7@dtest.test", "A", "507");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner7@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, owner_id, "secretary");

    let ownership_id = owned_ownership_id(&url, &admin_jwt, &owner_jwt);
    add_tenant(&url, &owner_jwt, ownership_id, "designtenant7@dtest.test").unwrap();
    let tenant_jwt = quick_onboard_user(&env, "designtenant7@dtest.test", "password123");

    let login: serde_json::Value = ureq::post(&format!("{}/api/v1/login", url))
        .send_json(serde_json::json!({
            "email": "designtenant7@dtest.test",
            "password": "password123"
        }))
        .unwrap()
        .into_json()
        .unwrap();

    assert_eq!(
        login["user"]["designations"],
        serde_json::json!([]),
        "the tenant account must hold none of the owner's designations"
    );
    assert_eq!(login["user"]["can_review_process"], false);

    // Access granted purely by the "secretary" designation must also not leak through:
    // the tenant has no scoped access to any process via that route.
    let process_list: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({ "personal_only": false }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        process_list["processes"].as_array().unwrap().len(),
        0,
        "the tenant must not see a global reviewer-style list just because the owner holds a designation"
    );
}
