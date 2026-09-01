//! End-to-end coverage for `StepOwner::Designation`-owned steps, through the real HTTP
//! API and a real database — not just the pure state-machine unit test. Covers the
//! rejection/acceptance rule itself, that it survives even for a global admin, that
//! `acted_as_designation` actually round-trips through the database, and that the
//! template-save auto-grant lets the designation's holder act on someone else's process.

use quality::{
    quick_assign_designation, quick_create_designation, quick_create_user, quick_onboard_user,
    quick_setup_admin, quick_toggle_designation_permission, setup_api_test_env,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn designation_owned_draft(designation_code: &str) -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Designation Ownership Test Process",
        "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Committee Note",
                "filled_by": { "kind": "designation", "code": designation_code },
                "fields": [
                    { "name": "Note", "type": "text", "filled_by": "user" }
                ]
            }
        ]
    })
}

fn create_and_activate_template(url: &str, admin_jwt: &str, draft: &serde_json::Value) -> i32 {
    let save_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
            .set("Authorization", &auth(admin_jwt))
            .send_json(draft.clone())
            .expect("save template")
            .into_json()
            .unwrap();
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
                "text_value": "Discussed at committee meeting.",
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
    // designation_owned_draft's stage carries its field directly (no nested substep).
    let step = &state["steps"][0];
    let step_id = step["template_step_id"].as_i64().unwrap();
    let field_id = step["fields"][0]["template_field_id"].as_i64().unwrap();
    (step_id, field_id)
}

/// The process owner themselves, lacking the step's designation, is rejected by the
/// real save-step API — not just by the pure `validate_save` unit test.
#[tokio::test]
async fn test_owner_without_designation_is_rejected_via_real_api() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");

    let template_id =
        create_and_activate_template(&url, &admin_jwt, &designation_owned_draft("secretary"));

    quick_create_user(&env, &admin_jwt, "owner_no_desig@dtest.test", "D", "101");
    let owner_jwt = quick_onboard_user(&env, "owner_no_desig@dtest.test", "password123");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let err = save_step(&url, &owner_jwt, process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        err, 400,
        "owning the process must not substitute for holding the designation"
    );
}

/// Confirms the exact claim in `ActorContext::may_act_on`'s doc comment through the real
/// API: holding `process.reviewer` globally does not grant access to a stage scoped to a
/// specific designation the caller doesn't hold, even on the admin's own process.
#[tokio::test]
async fn test_global_admin_without_designation_is_rejected_via_real_api() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");

    let template_id =
        create_and_activate_template(&url, &admin_jwt, &designation_owned_draft("secretary"));

    let state = start_process(&url, &admin_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let err = save_step(&url, &admin_jwt, process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        err, 400,
        "super_admin holds process.reviewer but not the 'secretary' designation"
    );
}

/// The actual designation holder can complete the step immediately (no PendingReview —
/// `Admin`/`Designation` owners never need a separate review step), and
/// `acted_as_designation` is readable back from a real database round-trip.
#[tokio::test]
async fn test_designation_holder_completes_step_and_acted_as_designation_persists() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");
    quick_toggle_designation_permission(&env, &admin_jwt, "secretary", "process.view", true);

    let template_id =
        create_and_activate_template(&url, &admin_jwt, &designation_owned_draft("secretary"));

    let holder_id = quick_create_user(&env, &admin_jwt, "secretary_holder@dtest.test", "D", "102");
    let holder_jwt = quick_onboard_user(&env, "secretary_holder@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, holder_id, "secretary");

    let state = start_process(&url, &holder_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let result = save_step(&url, &holder_jwt, process_id, step_id, field_id)
        .expect("the designation holder must be able to fill their own step");
    assert_eq!(result["status"], "success");

    let refetched: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&holder_jwt))
        .send_json(serde_json::json!({ "process_id": process_id }))
        .unwrap()
        .into_json()
        .unwrap();

    let step = &refetched["steps"][0];
    assert_eq!(
        step["status"], "COMPLETED",
        "a Designation-owned step completes immediately, no separate review needed"
    );
    let field = &step["fields"][0];
    let history_entry = &field["history"][0];
    assert_eq!(
        history_entry["acted_as_designation"], "secretary",
        "acted_as_designation must round-trip through process_fields and back out of get_field_history"
    );
}

/// The whole point of `StepOwner::Designation`: the designation's holder can act on a
/// stage in *someone else's* process — e.g. the Secretary reviewing a resident's
/// application — purely because the template save auto-granted `secretary` the same
/// `process.view`/`process.reviewer` rights as any reviewer. No manual permission grant,
/// no shared ownership.
#[tokio::test]
async fn test_non_owner_designation_holder_can_act_on_someone_elses_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");

    let template_id =
        create_and_activate_template(&url, &admin_jwt, &designation_owned_draft("secretary"));

    quick_create_user(&env, &admin_jwt, "third_party_owner@dtest.test", "D", "103");
    let owner_jwt = quick_onboard_user(&env, "third_party_owner@dtest.test", "password123");

    let holder_id = quick_create_user(
        &env,
        &admin_jwt,
        "third_party_secretary@dtest.test",
        "D",
        "104",
    );
    let holder_jwt = quick_onboard_user(&env, "third_party_secretary@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, holder_id, "secretary");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let result = save_step(&url, &holder_jwt, process_id, step_id, field_id)
        .expect("the designation's holder must be able to act on the owner's process");
    assert_eq!(result["status"], "success");
    assert_eq!(result["step"]["is_completed"], true);
}
