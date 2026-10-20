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
            },
            {
                "name": "Owner Declaration",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Declaration", "type": "declaration" }]
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
    env: &quality::TestEnv,
    admin_jwt: &str,
    owner_jwt: &str,
    ownership_id: i64,
    owner_wing: &str,
    owner_flat_number: &str,
    tenant_email: &str,
) -> Result<serde_json::Value, u16> {
    let url = env.server_url();
    let doc_id = quick_create_document(
        env,
        admin_jwt,
        "Rental Agreement",
        "RESIDENT_SPECIFIC",
        Some((owner_wing, owner_flat_number)),
    );
    let resp = ureq::post(&format!("{}/api/v1/tenants/add", url))
        .set("Authorization", &auth(owner_jwt))
        .send_json(serde_json::json!({
            "ownership_id": ownership_id,
            "tenant_email": tenant_email,
            "expires_at": "2027-01-01",
            "rental_agreement_document_id": doc_id
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

    let add_resp = add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "501",
        "tenant1@dtest.test",
    )
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
    assert_eq!(
        tenant_result["step"]["is_completed"], false,
        "a Tenant stage goes to review, not straight to completed"
    );

    let review = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": step_id,
            "is_approved": true
        }))
        .expect("an admin must be able to review a submitted Tenant stage")
        .into_json::<serde_json::Value>()
        .unwrap();
    assert_eq!(review["is_completed"], true);
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

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "502",
        "tenant2a@dtest.test",
    )
    .unwrap();
    let other_state = start_process(&url, &other_owner_jwt, template_id);
    let other_ownership_id = other_state["ownership_id"].as_i64().unwrap();
    add_tenant(
        &env,
        &admin_jwt,
        &other_owner_jwt,
        other_ownership_id,
        "B",
        "502",
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

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "503",
        "old_tenant@dtest.test",
    )
    .unwrap();
    let old_tenant_jwt = quick_onboard_user(&env, "old_tenant@dtest.test", "password123");

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "503",
        "new_tenant@dtest.test",
    )
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
        &env,
        &admin_jwt,
        &intruder_jwt,
        ownership_id,
        "A",
        "504",
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

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "505",
        "listtenant5@dtest.test",
    )
    .unwrap();
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
    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "506",
        "doctenant6@dtest.test",
    )
    .unwrap();
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
    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "507",
        "designtenant7@dtest.test",
    )
    .unwrap();
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

/// A `Tenant`-owned step on a flat with no tenant mapped yet would otherwise stall
/// forever — nobody, not even the owner, has standing on it. An admin may fill it as
/// an emergency override in that case, but the override disappears the moment a real
/// tenant is mapped.
#[tokio::test]
async fn test_admin_can_fill_tenant_step_only_when_no_tenant_is_mapped_yet() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner8@dtest.test", "A", "508");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner8@dtest.test", "password123");

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let ownership_id = state["ownership_id"].as_i64().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let result = save_step(&url, &admin_jwt, &process_id, step_id, field_id)
        .expect("an admin must be able to fill a Tenant-owned step when no tenant is mapped yet");
    assert_eq!(result["status"], "success");

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "508",
        "override8@dtest.test",
    )
    .unwrap();
    let tenant_jwt = quick_onboard_user(&env, "override8@dtest.test", "password123");

    let state2 = start_process(&url, &owner_jwt, template_id);
    let process_id2 = state2["process_id"].as_str().unwrap().to_string();
    let (step_id2, field_id2) = field_and_step_ids(&state2);

    let err = save_step(&url, &admin_jwt, &process_id2, step_id2, field_id2).unwrap_err();
    assert_eq!(
        err, 400,
        "once a real tenant is mapped, the admin override must disappear"
    );

    let tenant_result = save_step(&url, &tenant_jwt, &process_id2, step_id2, field_id2)
        .expect("the real tenant must still be able to act");
    assert_eq!(tenant_result["status"], "success");
}

/// Mapping an existing resident's or admin's email as a tenant would silently
/// cross-link their account with zero consent — rejected outright, even though
/// nothing would actually be inherited.
#[tokio::test]
async fn test_cannot_map_an_existing_privileged_account_as_a_tenant() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner9@dtest.test", "A", "509");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner9@dtest.test", "password123");
    let ownership_id = owned_ownership_id(&url, &admin_jwt, &owner_jwt);

    quick_create_user(&env, &admin_jwt, "other_owner9@dtest.test", "B", "509");
    quick_onboard_user(&env, "other_owner9@dtest.test", "password123");

    let err = add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "509",
        "other_owner9@dtest.test",
    )
    .unwrap_err();
    assert_eq!(
        err, 400,
        "an existing resident's account must not be silently cross-linked as a tenant"
    );

    quick_create_designation(&env, &admin_jwt, "committee9", "Committee");
    let staff_id = quick_create_user(&env, &admin_jwt, "staff_rep9@dtest.test", "C", "509");
    quick_onboard_user(&env, "staff_rep9@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, staff_id, "committee9");

    let err2 = add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "509",
        "staff_rep9@dtest.test",
    )
    .unwrap_err();
    assert_eq!(
        err2, 400,
        "an account holding a designation must not be silently cross-linked as a tenant either"
    );

    let ok = add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "509",
        "brand_new_tenant9@dtest.test",
    );
    assert!(
        ok.is_ok(),
        "a brand-new email with no existing account must still work"
    );
}

/// A tenant's scoped access to a process (via its own `Tenant`-owned step) must not
/// let them fill the owner's own `User`-owned steps in that same process.
#[tokio::test]
async fn test_tenant_cannot_fill_the_owners_user_owned_step() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner10@dtest.test", "A", "510");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner10@dtest.test", "password123");

    let draft = serde_json::json!({
        "template_id": null,
        "name": "Mixed Owner Test Process",
        "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Owner Stage",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Note", "type": "text" }]
            },
            {
                "name": "Tenant Stage",
                "filled_by": { "kind": "tenant" },
                "fields": [{ "name": "Note", "type": "text" }]
            },
            {
                "name": "Owner Declaration",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Declaration", "type": "declaration" }]
            }
        ]
    });
    let template_id = create_and_activate_template(&url, &admin_jwt, &draft);

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let ownership_id = state["ownership_id"].as_i64().unwrap();
    let owner_step_id = state["steps"][0]["template_step_id"].as_i64().unwrap();
    let owner_field_id = state["steps"][0]["fields"][0]["template_field_id"]
        .as_i64()
        .unwrap();

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "510",
        "scopedtenant10@dtest.test",
    )
    .unwrap();
    let tenant_jwt = quick_onboard_user(&env, "scopedtenant10@dtest.test", "password123");

    let err = save_step(
        &url,
        &tenant_jwt,
        &process_id,
        owner_step_id,
        owner_field_id,
    )
    .unwrap_err();
    assert_eq!(
        err, 400,
        "a tenant's scoped access to this process must not let them fill the owner's own User-owned step"
    );
}

/// A tenant's session falls back to their tenancy's flat, not 'TBD'.
#[tokio::test]
async fn test_tenant_login_shows_their_tenanted_flat_not_tbd() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner11@dtest.test", "A", "511");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner11@dtest.test", "password123");
    let ownership_id = owned_ownership_id(&url, &admin_jwt, &owner_jwt);

    add_tenant(
        &env,
        &admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        "511",
        "flatcheck11@dtest.test",
    )
    .unwrap();
    quick_onboard_user(&env, "flatcheck11@dtest.test", "password123");

    let login: serde_json::Value = ureq::post(&format!("{}/api/v1/login", url))
        .send_json(serde_json::json!({
            "email": "flatcheck11@dtest.test",
            "password": "password123"
        }))
        .unwrap()
        .into_json()
        .unwrap();

    assert_eq!(login["user"]["wing"], "A");
    assert_eq!(login["user"]["flat_number"], "0511");
}

/// An admin cannot approve an unfilled Open Tenant-owned step.
#[tokio::test]
async fn test_admin_cannot_approve_an_unfilled_open_tenant_step() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner12@dtest.test", "A", "512");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner12@dtest.test", "password123");

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let (step_id, _field_id) = field_and_step_ids(&state);

    let resp = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": step_id,
            "is_approved": true,
            "notes": null,
            "flagged_step_ids": null
        }));
    let err = match resp {
        Ok(_) => panic!("expected the approval to be rejected"),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("transport error: {:?}", e),
    };
    assert_eq!(
        err, 400,
        "an admin must not be able to approve an Open Tenant-owned step that was never filled"
    );
}

const DECLARATION_VALUE: &str = r#"{"agreed":true,"blanks":{}}"#;

fn owner_first_draft(name: &str) -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": name,
        "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Owner Declaration",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Declaration", "type": "declaration" }]
            },
            {
                "name": "Tenant Stage",
                "filled_by": { "kind": "tenant" },
                "fields": [{ "name": "Note", "type": "text" }]
            }
        ]
    })
}

fn save_value(
    url: &str,
    jwt: &str,
    process_id: &str,
    step_id: i64,
    field_id: i64,
    text_value: Option<&str>,
    document_id: Option<&str>,
) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": step_id,
            "fields": [{
                "template_field_id": field_id,
                "text_value": text_value,
                "document_id": document_id
            }]
        }));
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn template_save_status(url: &str, admin_jwt: &str, draft: &serde_json::Value) -> u16 {
    match ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(draft.clone())
    {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn onboard_owner_with_tenant(
    env: &quality::TestEnv,
    admin_jwt: &str,
    owner_email: &str,
    flat_number: &str,
    tenant_email: &str,
) -> (String, String) {
    let url = env.server_url();
    quick_create_user(env, admin_jwt, owner_email, "A", flat_number);
    let owner_jwt = quick_onboard_user(env, owner_email, "password123");
    let ownership_id = owned_ownership_id(&url, admin_jwt, &owner_jwt);
    add_tenant(
        env,
        admin_jwt,
        &owner_jwt,
        ownership_id,
        "A",
        flat_number,
        tenant_email,
    )
    .unwrap();
    let tenant_jwt = quick_onboard_user(env, tenant_email, "password123");
    (owner_jwt, tenant_jwt)
}

/// A tenant can start a process on a template with a Tenant stage; it belongs to the
/// flat's owner, so it shows up in the owner's list.
#[tokio::test]
async fn test_tenant_started_process_belongs_to_the_flat_owner() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner20@dtest.test",
        "520",
        "starter20@dtest.test",
    );

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state = start_process(&url, &tenant_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();

    let listed: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({ "personal_only": true }))
        .unwrap()
        .into_json()
        .unwrap();
    let ids: Vec<&str> = listed["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["process_id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&process_id.as_str()),
        "a tenant-started process must appear in the owner's list"
    );
}

/// The owner's declaration is the owner's alone: not the tenant who started the
/// process, not an admin, not another flat's owner.
#[tokio::test]
async fn test_only_the_flat_owner_can_make_the_owner_declaration() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner21@dtest.test",
        "521",
        "starter21@dtest.test",
    );
    quick_create_user(&env, &admin_jwt, "tenant_owner21b@dtest.test", "B", "521");
    let other_owner_jwt = quick_onboard_user(&env, "tenant_owner21b@dtest.test", "password123");

    let template_id =
        create_and_activate_template(&url, &admin_jwt, &owner_first_draft("Declaration Gate"));
    let state = start_process(&url, &tenant_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let (step_id, field_id) = field_and_step_ids(&state);

    let attempt = |jwt: &str| {
        save_value(
            &url,
            jwt,
            &process_id,
            step_id,
            field_id,
            Some(DECLARATION_VALUE),
            None,
        )
    };
    assert_eq!(
        attempt(&tenant_jwt).unwrap_err(),
        400,
        "tenant must not declare"
    );
    assert_eq!(
        attempt(&admin_jwt).unwrap_err(),
        400,
        "admin must not declare"
    );
    assert_eq!(
        attempt(&other_owner_jwt).unwrap_err(),
        403,
        "another flat's owner has no standing"
    );
    let done = attempt(&owner_jwt).expect("the flat's owner must be able to declare");
    assert_eq!(done["status"], "success");
}

/// A template with a Tenant stage but no required owner declaration can't be saved.
#[tokio::test]
async fn test_template_with_tenant_stage_needs_an_owner_declaration() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let no_declaration = serde_json::json!({
        "template_id": null, "name": "No Owner Declaration", "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Tenant Stage",
                "filled_by": { "kind": "tenant" },
                "fields": [{ "name": "Note", "type": "text" }]
            },
            {
                "name": "Owner Note",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Note", "type": "text" }]
            }
        ]
    });
    assert_eq!(template_save_status(&url, &admin_jwt, &no_declaration), 400);

    let optional_declaration = serde_json::json!({
        "template_id": null, "name": "Optional Owner Declaration", "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Tenant Stage",
                "filled_by": { "kind": "tenant" },
                "fields": [{ "name": "Note", "type": "text" }]
            },
            {
                "name": "Owner Declaration",
                "filled_by": { "kind": "user" },
                "is_optional": true,
                "fields": [{ "name": "Declaration", "type": "declaration" }]
            }
        ]
    });
    assert_eq!(
        template_save_status(&url, &admin_jwt, &optional_declaration),
        400
    );

    assert_eq!(template_save_status(&url, &admin_jwt, &tenant_draft()), 200);
}

/// A tenant can only start templates that have a Tenant stage, and only sees those in
/// the picker; the owner sees every template.
#[tokio::test]
async fn test_tenant_start_and_picker_are_limited_to_tenant_templates() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner22@dtest.test",
        "522",
        "starter22@dtest.test",
    );

    let owner_only = serde_json::json!({
        "template_id": null, "name": "Owner Only Picker", "description": null,
        "is_optional": false,
        "substeps": [{"name": "S", "fields": [{"name": "N", "type": "text"}]}]
    });
    let owner_only_id = create_and_activate_template(&url, &admin_jwt, &owner_only);
    let tenant_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());

    let start = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({ "template_id": owner_only_id }));
    assert!(matches!(start, Err(ureq::Error::Status(403, _))));

    let ids_for = |jwt: &str| -> Vec<i64> {
        let templates: Vec<serde_json::Value> =
            ureq::post(&format!("{}/api/v1/process-templates/available", url))
                .set("Authorization", &auth(jwt))
                .send_json(serde_json::json!({}))
                .unwrap()
                .into_json()
                .unwrap();
        templates
            .iter()
            .map(|t| t["template_id"].as_i64().unwrap())
            .collect()
    };
    let tenant_ids = ids_for(&tenant_jwt);
    assert!(tenant_ids.contains(&(tenant_id as i64)));
    assert!(!tenant_ids.contains(&(owner_only_id as i64)));
    let owner_ids = ids_for(&owner_jwt);
    assert!(owner_ids.contains(&(tenant_id as i64)));
    assert!(owner_ids.contains(&(owner_only_id as i64)));
}

/// A tenant attaches a document they uploaded to their Tenant stage; the flat's owner
/// can download it, another flat's owner cannot.
#[tokio::test]
async fn test_tenant_can_attach_own_document_and_only_the_flat_owner_can_download_it() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner23@dtest.test",
        "523",
        "starter23@dtest.test",
    );
    quick_create_user(&env, &admin_jwt, "tenant_owner23b@dtest.test", "B", "523");
    let other_owner_jwt = quick_onboard_user(&env, "tenant_owner23b@dtest.test", "password123");

    let draft = serde_json::json!({
        "template_id": null, "name": "Tenant KYC Upload", "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Tenant KYC",
                "filled_by": { "kind": "tenant" },
                "fields": [{ "name": "KYC", "type": "document" }]
            },
            {
                "name": "Owner Declaration",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Declaration", "type": "declaration" }]
            }
        ]
    });
    let template_id = create_and_activate_template(&url, &admin_jwt, &draft);
    let state = start_process(&url, &tenant_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();
    let (step_id, field_id) = field_and_step_ids(&state);

    let upload: serde_json::Value = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({
            "title": "KYC.pdf", "filename": "KYC.pdf", "content_type": "application/pdf"
        }))
        .expect("a tenant must be able to request an upload for their own flat")
        .into_json()
        .unwrap();
    let document_id = upload["document_id"].as_str().unwrap().to_string();
    ureq::put(upload["upload_url"].as_str().unwrap())
        .set("Content-Type", "application/pdf")
        .send_bytes(b"Simulated PDF content bytes for testing")
        .expect("S3 PUT upload failed");
    ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({ "document_id": document_id }))
        .expect("a tenant must be able to confirm their own upload");

    save_value(
        &url,
        &tenant_jwt,
        &process_id,
        step_id,
        field_id,
        None,
        Some(&document_id),
    )
    .expect("a tenant must be able to attach their own document");

    let download = |jwt: &str| -> u16 {
        match ureq::post(&format!("{}/api/v1/process/download-url", url))
            .set("Authorization", &auth(jwt))
            .send_json(serde_json::json!({
                "process_id": process_id,
                "document_id": document_id
            })) {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("transport error: {:?}", e),
        }
    };
    assert_eq!(
        download(&owner_jwt),
        200,
        "the flat's owner must be able to download"
    );
    assert_eq!(download(&other_owner_jwt), 403);
}

fn post_status(url: &str, path: &str, jwt: &str, body: serde_json::Value) -> u16 {
    match ureq::post(&format!("{}{}", url, path))
        .set("Authorization", &auth(jwt))
        .send_json(body)
    {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn listed_process_ids(url: &str, jwt: &str) -> Vec<String> {
    let listed: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "personal_only": true }))
        .unwrap()
        .into_json()
        .unwrap();
    listed["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["process_id"].as_str().unwrap().to_string())
        .collect()
}

/// One owner, two flats, a different tenant in each: the owner sees both flats'
/// processes, while each tenant sees and can touch only their own flat's.
#[tokio::test]
async fn test_owner_of_two_flats_sees_both_but_each_tenant_sees_only_their_own() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "tenant_owner24@dtest.test", "A", "701");
    quick_create_user(&env, &admin_jwt, "tenant_owner24@dtest.test", "A", "702");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner24@dtest.test", "password123");

    let my_flats: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/tenants/my-flats", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let ownership_of = |flat_number: &str| -> i64 {
        my_flats
            .iter()
            .find(|f| f["flat_number"].as_str().unwrap().ends_with(flat_number))
            .and_then(|f| f["ownership_id"].as_i64())
            .expect("ownership for flat")
    };

    for (flat_number, tenant_email) in [
        ("701", "tenant24a@dtest.test"),
        ("702", "tenant24b@dtest.test"),
    ] {
        add_tenant(
            &env,
            &admin_jwt,
            &owner_jwt,
            ownership_of(flat_number),
            "A",
            flat_number,
            tenant_email,
        )
        .unwrap();
    }
    let tenant_a_jwt = quick_onboard_user(&env, "tenant24a@dtest.test", "password123");
    let tenant_b_jwt = quick_onboard_user(&env, "tenant24b@dtest.test", "password123");

    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state_a = start_process(&url, &tenant_a_jwt, template_id);
    let state_b = start_process(&url, &tenant_b_jwt, template_id);
    let process_a = state_a["process_id"].as_str().unwrap().to_string();
    let process_b = state_b["process_id"].as_str().unwrap().to_string();
    assert_eq!(state_a["ownership_id"].as_i64(), Some(ownership_of("701")));
    assert_eq!(state_b["ownership_id"].as_i64(), Some(ownership_of("702")));
    let (step_a, field_a) = field_and_step_ids(&state_a);
    let (step_b, field_b) = field_and_step_ids(&state_b);

    let owner_sees = listed_process_ids(&url, &owner_jwt);
    assert!(owner_sees.contains(&process_a) && owner_sees.contains(&process_b));

    let a_sees = listed_process_ids(&url, &tenant_a_jwt);
    assert!(a_sees.contains(&process_a) && !a_sees.contains(&process_b));
    let b_sees = listed_process_ids(&url, &tenant_b_jwt);
    assert!(b_sees.contains(&process_b) && !b_sees.contains(&process_a));

    let state_of = |jwt: &str, process_id: &str| {
        post_status(
            &url,
            "/api/v1/process/state",
            jwt,
            serde_json::json!({ "process_id": process_id }),
        )
    };
    assert_eq!(state_of(&tenant_a_jwt, &process_b), 403);
    assert_eq!(state_of(&tenant_b_jwt, &process_a), 403);
    assert_eq!(state_of(&owner_jwt, &process_a), 200);
    assert_eq!(state_of(&owner_jwt, &process_b), 200);

    assert_eq!(
        save_step(&url, &tenant_a_jwt, &process_b, step_b, field_b).unwrap_err(),
        403
    );
    assert_eq!(
        save_step(&url, &tenant_b_jwt, &process_a, step_a, field_a).unwrap_err(),
        403
    );
    assert!(save_step(&url, &tenant_a_jwt, &process_a, step_a, field_a).is_ok());
    assert!(save_step(&url, &tenant_b_jwt, &process_b, step_b, field_b).is_ok());

    let flats_of = |jwt: &str| -> Vec<String> {
        let listed: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
            .set("Authorization", &auth(jwt))
            .send_json(serde_json::json!({ "personal_only": true }))
            .unwrap()
            .into_json()
            .unwrap();
        listed["flats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["flat_number"].as_str().unwrap().to_string())
            .collect()
    };
    let a_flats = flats_of(&tenant_a_jwt);
    assert_eq!(a_flats.len(), 1);
    assert!(a_flats[0].ends_with("701"));
    let b_flats = flats_of(&tenant_b_jwt);
    assert_eq!(b_flats.len(), 1);
    assert!(b_flats[0].ends_with("702"));
    assert_eq!(flats_of(&owner_jwt).len(), 2);
}

fn self_upload(url: &str, jwt: &str, filename: &str) -> String {
    let upload: serde_json::Value = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({
            "title": filename, "filename": filename, "content_type": "application/pdf"
        }))
        .expect("upload-url request failed")
        .into_json()
        .unwrap();
    let document_id = upload["document_id"].as_str().unwrap().to_string();
    ureq::put(upload["upload_url"].as_str().unwrap())
        .set("Content-Type", "application/pdf")
        .send_bytes(b"Simulated PDF content bytes for testing")
        .expect("S3 PUT upload failed");
    ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "document_id": document_id }))
        .expect("confirm-upload request failed");
    document_id
}

/// A tenant sees the data of their own stage only — never the owner's field values,
/// documents, or review notes — and cannot download the owner's documents.
#[tokio::test]
async fn test_tenant_sees_and_downloads_only_tenant_stage_data() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner25@dtest.test",
        "525",
        "starter25@dtest.test",
    );

    let draft = serde_json::json!({
        "template_id": null, "name": "Redaction Process", "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Owner Stage",
                "filled_by": { "kind": "user" },
                "fields": [
                    { "name": "Secret", "type": "text" },
                    { "name": "Owner Doc", "type": "document" }
                ]
            },
            {
                "name": "Tenant Stage",
                "filled_by": { "kind": "tenant" },
                "fields": [{ "name": "Note", "type": "text" }]
            },
            {
                "name": "Owner Declaration",
                "filled_by": { "kind": "user" },
                "fields": [{ "name": "Declaration", "type": "declaration" }]
            }
        ]
    });
    let template_id = create_and_activate_template(&url, &admin_jwt, &draft);
    let state = start_process(&url, &tenant_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();

    let owner_state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({ "process_id": process_id }))
        .unwrap()
        .into_json()
        .unwrap();
    let owner_step = &owner_state["steps"][0];
    let owner_step_id = owner_step["template_step_id"].as_i64().unwrap();
    let secret_field = owner_step["fields"][0]["template_field_id"]
        .as_i64()
        .unwrap();
    let doc_field = owner_step["fields"][1]["template_field_id"]
        .as_i64()
        .unwrap();

    let owner_doc = self_upload(&url, &owner_jwt, "owner_id.pdf");
    let saved = ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": owner_step_id,
            "fields": [
                { "template_field_id": secret_field, "text_value": "OWNER-SECRET", "document_id": null },
                { "template_field_id": doc_field, "text_value": null, "document_id": owner_doc }
            ]
        }));
    assert!(
        saved.is_ok(),
        "the owner must be able to fill their own stage"
    );

    ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": owner_step_id,
            "is_approved": true,
            "notes": "OWNER-REVIEW-NOTE"
        }))
        .expect("admin review");

    let tenant_view: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({ "process_id": process_id }))
        .unwrap()
        .into_json()
        .unwrap();
    let raw = tenant_view.to_string();
    assert!(
        !raw.contains("OWNER-SECRET") && !raw.contains("OWNER-REVIEW-NOTE"),
        "the tenant must never receive the owner's stage values or review notes"
    );
    assert!(!raw.contains(&owner_doc), "nor the owner's document id");
    assert_eq!(
        tenant_view["steps"][1]["status"], "OPEN",
        "the tenant's own stage still unlocks once the owner's is approved"
    );

    let owner_raw = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({ "process_id": process_id }))
        .unwrap()
        .into_string()
        .unwrap();
    assert!(
        owner_raw.contains("OWNER-SECRET"),
        "the owner still sees their own data"
    );

    let download = |jwt: &str| {
        post_status(
            &url,
            "/api/v1/process/download-url",
            jwt,
            serde_json::json!({ "process_id": process_id, "document_id": owner_doc }),
        )
    };
    assert_eq!(download(&owner_jwt), 200);
    assert_eq!(
        download(&tenant_jwt),
        403,
        "a tenant must not download the owner's document"
    );
}

/// The admin upload routes are for reviewers; neither the owner nor the tenant of the
/// process may use them.
#[tokio::test]
async fn test_residents_cannot_use_the_admin_process_upload_routes() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner26@dtest.test",
        "526",
        "starter26@dtest.test",
    );
    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let state = start_process(&url, &tenant_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();

    for jwt in [&owner_jwt, &tenant_jwt] {
        assert_eq!(
            post_status(
                &url,
                "/api/v1/admin/process/upload-url",
                jwt,
                serde_json::json!({
                    "process_id": process_id, "title": "x",
                    "filename": "x.pdf", "content_type": "application/pdf"
                }),
            ),
            403
        );
        assert_eq!(
            post_status(
                &url,
                "/api/v1/admin/process/confirm-upload",
                jwt,
                serde_json::json!({
                    "process_id": process_id,
                    "document_id": "00000000-0000-0000-0000-000000000000"
                }),
            ),
            403
        );
    }
}

/// The tenant's process list never carries the owner's email.
#[tokio::test]
async fn test_tenant_process_list_does_not_expose_the_owners_email() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let (_owner_jwt, tenant_jwt) = onboard_owner_with_tenant(
        &env,
        &admin_jwt,
        "tenant_owner27@dtest.test",
        "527",
        "starter27@dtest.test",
    );
    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    start_process(&url, &tenant_jwt, template_id);

    let listed = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &auth(&tenant_jwt))
        .send_json(serde_json::json!({ "personal_only": true }))
        .unwrap()
        .into_string()
        .unwrap();
    assert!(listed.contains("process_id"));
    assert!(!listed.contains("tenant_owner27@dtest.test"));
}

/// The admin process upload route enforces the same 200-character title cap as the
/// resident upload path.
#[tokio::test]
async fn test_admin_process_upload_title_is_length_capped() {
    let env = quality::setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "tenant_owner28@dtest.test", "A", "528");
    let owner_jwt = quick_onboard_user(&env, "tenant_owner28@dtest.test", "password123");
    let template_id = create_and_activate_template(&url, &admin_jwt, &tenant_draft());
    let process_id = start_process(&url, &owner_jwt, template_id)["process_id"]
        .as_str()
        .unwrap()
        .to_string();

    let upload = |title: String| {
        post_status(
            &url,
            "/api/v1/admin/process/upload-url",
            &admin_jwt,
            serde_json::json!({
                "process_id": process_id, "title": title,
                "filename": "x.pdf", "content_type": "application/pdf"
            }),
        )
    };
    assert_eq!(upload("a".repeat(201)), 400);
    assert_eq!(upload("   ".to_string()), 400);
    assert_eq!(upload("ok title".to_string()), 201);
}
