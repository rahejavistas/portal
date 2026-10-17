//! End-to-end coverage for `StepOwner::Designation`-owned steps, through the real HTTP
//! API and a real database — not just the pure state-machine unit test. Covers the
//! rejection/acceptance rule itself, that it survives even for a global admin, that
//! `acted_as_designation` actually round-trips through the database, and that a
//! designation holder gets a scoped access path onto someone else's process without
//! ever needing global `process.reviewer`/`process.view`.

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
/// application — purely by holding the designation itself, with no `process.view`/
/// `process.reviewer` permission grant of any kind and no shared ownership.
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

fn wing_representative_draft() -> serde_json::Value {
    serde_json::json!({
        "template_id": null,
        "name": "Wing Representative Test Process",
        "description": null,
        "is_optional": false,
        "substeps": [
            {
                "name": "Wing Sign-off",
                "filled_by": { "kind": "wing_representative" },
                "fields": [
                    { "name": "Note", "type": "text", "filled_by": "user" }
                ]
            }
        ]
    })
}

/// `WingRepresentative` resolves to the resident-owner's OWN wing's designation —
/// never a fixed, template-authored code. A different wing's own representative must
/// have zero standing on it.
#[tokio::test]
async fn test_wing_representative_resolves_to_owners_own_wing_and_excludes_other_wings() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    let _owner_id = quick_create_user(&env, &admin_jwt, "wing_a_owner@dtest.test", "A", "201");
    let owner_jwt = quick_onboard_user(&env, "wing_a_owner@dtest.test", "password123");

    let a_rep_id = quick_create_user(&env, &admin_jwt, "wing_a_rep@dtest.test", "A", "202");
    let a_rep_jwt = quick_onboard_user(&env, "wing_a_rep@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, a_rep_id, "wing_rep_a");

    let b_rep_id = quick_create_user(&env, &admin_jwt, "wing_b_rep@dtest.test", "B", "301");
    let b_rep_jwt = quick_onboard_user(&env, "wing_b_rep@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, b_rep_id, "wing_rep_b");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let err = save_step(&url, &b_rep_jwt, process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        err, 403,
        "wing B's own representative has no standing on wing A's resident's process"
    );

    let result = save_step(&url, &a_rep_jwt, process_id, step_id, field_id)
        .expect("wing A's own representative must be able to act");
    assert_eq!(result["status"], "success");
    assert_eq!(result["step"]["is_completed"], true);
}

fn revoke_step(
    url: &str,
    jwt: &str,
    process_id: &str,
    template_step_id: i64,
) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!("{}/api/v1/admin/process/revoke", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": template_step_id,
            "notes": null
        }));
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

/// A holder of a completely unrelated designation (not a wing rep at all) has zero
/// standing on a `WingRepresentative`-owned stage — not just "the wrong wing".
#[tokio::test]
async fn test_wing_representative_excludes_unrelated_designation() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    let _owner_id = quick_create_user(&env, &admin_jwt, "wing_a_owner2@dtest.test", "A", "203");
    let owner_jwt = quick_onboard_user(&env, "wing_a_owner2@dtest.test", "password123");

    let secretary_id = quick_create_user(&env, &admin_jwt, "secretary_x@dtest.test", "C", "401");
    let secretary_jwt = quick_onboard_user(&env, "secretary_x@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, secretary_id, "secretary");

    let a_rep_id = quick_create_user(&env, &admin_jwt, "wing_a_rep2@dtest.test", "A", "204");
    let a_rep_jwt = quick_onboard_user(&env, "wing_a_rep2@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, a_rep_id, "wing_rep_a");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let err = save_step(&url, &secretary_jwt, process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        err, 403,
        "an unrelated designation (secretary) has no standing on a wing-rep stage"
    );

    let result = save_step(&url, &a_rep_jwt, process_id, step_id, field_id)
        .expect("wing A's own representative must still be able to act");
    assert_eq!(result["status"], "success");
}

/// A global admin CAN reopen a completed `WingRepresentative` stage (the review/reopen
/// path deliberately allows an admin override, per `ActorContext::may_review`), but a
/// holder of an unrelated designation cannot.
#[tokio::test]
async fn test_wing_representative_completed_step_reopen_exclusivity() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    let _owner_id = quick_create_user(&env, &admin_jwt, "wing_a_owner3@dtest.test", "A", "205");
    let owner_jwt = quick_onboard_user(&env, "wing_a_owner3@dtest.test", "password123");

    let secretary_id = quick_create_user(&env, &admin_jwt, "secretary_y@dtest.test", "C", "402");
    let secretary_jwt = quick_onboard_user(&env, "secretary_y@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, secretary_id, "secretary");

    let a_rep_id = quick_create_user(&env, &admin_jwt, "wing_a_rep3@dtest.test", "A", "206");
    let a_rep_jwt = quick_onboard_user(&env, "wing_a_rep3@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, a_rep_id, "wing_rep_a");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    save_step(&url, &a_rep_jwt, process_id, step_id, field_id)
        .expect("wing A's rep completes the step");

    let err = revoke_step(&url, &secretary_jwt, process_id, step_id).unwrap_err();
    assert_eq!(
        err, 403,
        "an unrelated designation cannot reopen a wing-rep-owned step"
    );

    let result = revoke_step(&url, &admin_jwt, process_id, step_id)
        .expect("a global admin can reopen any step, including a wing-rep-owned one");
    assert_eq!(result["status"], "success");
}

/// Reproduces the exact race this feature hit in production: the stage opens BEFORE
/// anyone holds the wing's designation. Assigning it afterward must not error, and the
/// newly-assigned holder must immediately be able to act — proving
/// `notify_designation_of_currently_open_steps` (the catch-up notification) never
/// breaks the assignment flow even though no email can be asserted without an SMTP mock.
#[tokio::test]
async fn test_assigning_wing_representative_designation_after_step_already_open() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    let _owner_id = quick_create_user(&env, &admin_jwt, "wing_a_owner4@dtest.test", "A", "207");
    let owner_jwt = quick_onboard_user(&env, "wing_a_owner4@dtest.test", "password123");

    // The stage opens here — nobody holds "wing_rep_a" yet.
    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let late_rep_id = quick_create_user(&env, &admin_jwt, "late_wing_a_rep@dtest.test", "A", "208");
    let late_rep_jwt = quick_onboard_user(&env, "late_wing_a_rep@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, late_rep_id, "wing_rep_a");

    let result = save_step(&url, &late_rep_jwt, process_id, step_id, field_id)
        .expect("the late-assigned holder must still be able to act on the already-open stage");
    assert_eq!(result["status"], "success");
}
