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

/// Any wing's representative may act on a `WingRepresentative`-owned step, regardless
/// of which wing the process's own flat belongs to — the per-wing representative
/// *assignment* stays wing-scoped, but the authorization check itself is not.
#[tokio::test]
async fn test_any_wing_representative_can_act_on_any_wings_process() {
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

    let result = save_step(&url, &b_rep_jwt, process_id, step_id, field_id)
        .expect("wing B's own representative must also be able to act on wing A's process");
    assert_eq!(result["status"], "success");
    assert_eq!(result["step"]["is_completed"], true);

    let second_owner_jwt = {
        quick_create_user(&env, &admin_jwt, "wing_a_owner2@dtest.test", "A", "213");
        quick_onboard_user(&env, "wing_a_owner2@dtest.test", "password123")
    };
    let second_state = start_process(&url, &second_owner_jwt, template_id);
    let second_process_id = second_state["process_id"].as_str().unwrap();
    let (second_step_id, second_field_id) = field_and_step_ids(&second_state);

    let result = save_step(
        &url,
        &a_rep_jwt,
        second_process_id,
        second_step_id,
        second_field_id,
    )
    .expect("wing A's own representative must still be able to act");
    assert_eq!(result["status"], "success");
}

/// A caller holding no wing-representative designation at all (not even for their own
/// wing) still has zero standing — the check grants access to any wing's *rep*, not to
/// every resident.
#[tokio::test]
async fn test_plain_resident_without_any_wing_representative_designation_is_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    let _owner_id = quick_create_user(
        &env,
        &admin_jwt,
        "wing_a_owner_plain@dtest.test",
        "A",
        "212",
    );
    let owner_jwt = quick_onboard_user(&env, "wing_a_owner_plain@dtest.test", "password123");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    let err = save_step(&url, &owner_jwt, process_id, step_id, field_id).unwrap_err();
    assert_eq!(
        err, 400,
        "the process's own owner, holding no wing-rep designation, must not be able to act"
    );
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

fn list_processes(url: &str, jwt: &str) -> serde_json::Value {
    ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "personal_only": false }))
        .expect("list processes")
        .into_json()
        .unwrap()
}

/// A plain designation holder's list is scoped to only what's pending on them.
#[tokio::test]
async fn test_process_list_scopes_plain_designation_holder_to_their_own_pending_work() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let wing_template_id =
        create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());
    let secretary_template_id =
        create_and_activate_template(&url, &admin_jwt, &designation_owned_draft("secretary"));
    quick_create_designation(&env, &admin_jwt, "secretary", "Secretary");

    let _wing_owner_id =
        quick_create_user(&env, &admin_jwt, "list_wing_owner@dtest.test", "A", "209");
    let wing_owner_jwt = quick_onboard_user(&env, "list_wing_owner@dtest.test", "password123");
    let wing_state = start_process(&url, &wing_owner_jwt, wing_template_id);
    let wing_process_id = wing_state["process_id"].as_str().unwrap().to_string();

    let _secretary_owner_id = quick_create_user(
        &env,
        &admin_jwt,
        "list_secretary_owner@dtest.test",
        "D",
        "210",
    );
    let secretary_owner_jwt =
        quick_onboard_user(&env, "list_secretary_owner@dtest.test", "password123");
    let secretary_state = start_process(&url, &secretary_owner_jwt, secretary_template_id);
    let secretary_process_id = secretary_state["process_id"].as_str().unwrap().to_string();

    let a_rep_id = quick_create_user(&env, &admin_jwt, "list_wing_a_rep@dtest.test", "A", "211");
    let a_rep_jwt = quick_onboard_user(&env, "list_wing_a_rep@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, a_rep_id, "wing_rep_a");

    let listed = list_processes(&url, &a_rep_jwt);
    let processes = listed["processes"].as_array().unwrap();
    let process_ids: Vec<&str> = processes
        .iter()
        .map(|p| p["process_id"].as_str().unwrap())
        .collect();

    assert!(
        process_ids.contains(&wing_process_id.as_str()),
        "wing A's rep must see the process pending on their own designation"
    );
    assert!(
        !process_ids.contains(&secretary_process_id.as_str()),
        "wing A's rep must NOT see an unrelated resident's process owned by a different designation"
    );
    assert_eq!(
        processes.len(),
        1,
        "a plain designation holder's list must contain only what's pending on them, nothing else"
    );
    assert_eq!(processes[0]["pending_on_caller"], true);
}

/// A real global reviewer still sees everyone's processes (unchanged), with
/// `pending_on_caller` correctly set per item instead of being filtered away.
#[tokio::test]
async fn test_process_list_shows_full_list_with_pending_flag_for_global_reviewer() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());
    let _owner_id = quick_create_user(&env, &admin_jwt, "list_owner2@dtest.test", "A", "212");
    let owner_jwt = quick_onboard_user(&env, "list_owner2@dtest.test", "password123");
    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap().to_string();

    let listed = list_processes(&url, &admin_jwt);
    let processes = listed["processes"].as_array().unwrap();
    let entry = processes
        .iter()
        .find(|p| p["process_id"].as_str() == Some(process_id.as_str()))
        .expect("super_admin must see every process, including this one");

    // super_admin holds process.reviewer globally but not the "wing_rep_a" designation —
    // exclusivity means it's still not pending on them specifically.
    assert_eq!(entry["pending_on_caller"], false);
}

fn review_cascade_template() -> process::ProcessTemplate {
    let note_field = |field_id: i32| process::ProcessFieldTemplate {
        field_id,
        name: "Note".to_string(),
        description: None,
        r#type: process::FieldType::Text,
        is_optional: false,
        requirements: None,
        requires_signature: false,
        required_signs_from_designations: vec![],
        options: None,
        example_document_id: None,
    };
    process::ProcessTemplate {
        name: "Review Cascade Test".to_string(),
        description: None,
        step_id: 1,
        is_optional: false,
        fields: None,
        tat_working_days: None,
        filled_by: process::StepOwner::User,
        substeps: Some(vec![
            process::ProcessStepTemplate {
                step_id: 2,
                name: "Resident Stage".to_string(),
                description: None,
                is_optional: false,
                filled_by: process::StepOwner::User,
                fields: Some(vec![note_field(201)]),
                substeps: None,
            },
            process::ProcessStepTemplate {
                step_id: 3,
                name: "Designation Stage".to_string(),
                description: None,
                is_optional: false,
                filled_by: process::StepOwner::Designation {
                    code: "secretary".to_string(),
                },
                fields: Some(vec![note_field(202)]),
                substeps: None,
            },
        ]),
    }
}

async fn seed_template_version(
    conn: &mut sqlx::PgConnection,
    template: &process::ProcessTemplate,
) -> i32 {
    let template_id: i32 = sqlx::query_scalar(
        "INSERT INTO process_templates (name) VALUES ($1) RETURNING template_id",
    )
    .bind(&template.name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let definition = serde_json::to_value(template).unwrap();
    sqlx::query_scalar(
        "INSERT INTO process_template_versions (template_id, version_number, definition) \
         VALUES ($1, 1, $2) RETURNING version_id",
    )
    .bind(template_id)
    .bind(&definition)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// First-time approval must report the cascaded Designation stage as newly-opened.
#[tokio::test]
async fn test_review_step_reports_cascaded_designation_stage_as_newly_opened() {
    let env = quality::setup_db_test_env().await;
    let pool = env.db_pool.clone();
    let mut conn = pool.acquire().await.unwrap();

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('review_cascade_user@vistas.com', TRUE) \
         RETURNING user_id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let template = review_cascade_template();
    let version_id = seed_template_version(&mut conn, &template).await;
    let process_id = process::db::init_process(&mut conn, user_id, version_id, &template, None)
        .await
        .unwrap()
        .process_id;

    let actor = process::state_machine::ActorContext {
        is_admin: false,
        designation_codes: Default::default(),
        ..Default::default()
    };
    process::db::save_step(
        &mut conn,
        process_id,
        &template,
        2,
        &[process::state_machine::SaveFieldInput {
            template_field_id: 201,
            text_value: Some("done".to_string()),
            document_id: None,
        }],
        &actor,
        user_id,
    )
    .await
    .expect("resident fills their stage");

    let statuses = process::db::get_statuses(&mut conn, process_id, &template)
        .await
        .unwrap();
    assert_eq!(
        statuses.get(&2),
        Some(&process::state_machine::StepStatus::PendingReview),
        "a top-level User-owned stage needs admin review before completing"
    );
    assert_eq!(
        statuses.get(&3),
        Some(&process::state_machine::StepStatus::Locked),
        "the designation stage must still be locked before stage 2 is approved"
    );

    let admin_actor = process::state_machine::ActorContext {
        is_admin: true,
        designation_codes: Default::default(),
        ..Default::default()
    };
    let outcome = process::db::review_step(
        &mut conn,
        process_id,
        &template,
        process::db::ReviewStepParams {
            step_id: 2,
            reviewer_user_id: Some(user_id),
            is_approved: true,
            notes: None,
            flagged_step_ids: &[],
            acted_as_designation: None,
        },
        &admin_actor,
    )
    .await
    .expect("admin approves the resident's stage");

    assert!(
        outcome.newly_opened_step_ids.contains(&3),
        "approving stage 2 for the first time must report stage 3 (designation-owned) as \
         newly opened, or its 'your turn' notification silently never fires: got {:?}",
        outcome.newly_opened_step_ids
    );
}

/// An admin-chosen designation code can't collide with the auto-provisioned wing-rep
/// slug and silently inherit standing on that wing's stages.
#[tokio::test]
async fn test_create_designation_rejects_reserved_wing_rep_prefix() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({ "code": "wing_rep_x", "label": "Totally Unrelated" }));
    match resp {
        Ok(_) => panic!("a 'wing_rep_' prefixed code must be rejected, not accepted"),
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

/// A wing can never be left without a possible representative: its auto-provisioned
/// designation must not be deletable even while nobody currently holds it.
#[tokio::test]
async fn test_cannot_delete_a_wings_representative_designation() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    // Provisions wing "Z" and its "wing_rep_z" designation; nobody holds it yet.
    quick_create_user(&env, &admin_jwt, "wing_z_owner@dtest.test", "Z", "901");

    let resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/delete", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({ "code": "wing_rep_z" }));
    match resp {
        Ok(_) => panic!("a wing's representative designation must not be deletable"),
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

/// A plain resident with no designation and no admin permission must never see another
/// resident's process via `/process/list`, even when explicitly asking for the full list.
#[tokio::test]
async fn test_plain_resident_list_excludes_other_residents_processes_even_with_personal_only_false()
{
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    quick_create_user(&env, &admin_jwt, "list_resident_a@dtest.test", "A", "301");
    let a_jwt = quick_onboard_user(&env, "list_resident_a@dtest.test", "password123");
    let a_state = start_process(&url, &a_jwt, template_id);
    let a_process_id = a_state["process_id"].as_str().unwrap().to_string();

    quick_create_user(&env, &admin_jwt, "list_resident_b@dtest.test", "A", "302");
    let b_jwt = quick_onboard_user(&env, "list_resident_b@dtest.test", "password123");

    let listed = list_processes(&url, &b_jwt);
    let processes = listed["processes"].as_array().unwrap();
    assert!(
        processes
            .iter()
            .all(|p| p["process_id"].as_str() != Some(a_process_id.as_str())),
        "a plain resident must never see another resident's process, personal_only or not"
    );
}

/// Wing A's representative also sees wing B's resident's process in their own list —
/// any wing's rep has standing on a `WingRepresentative`-owned step, regardless of
/// which wing the process's own flat belongs to.
#[tokio::test]
async fn test_wing_representative_list_includes_a_different_wings_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    quick_create_user(&env, &admin_jwt, "list_wing_a_owner@dtest.test", "A", "303");
    let a_owner_jwt = quick_onboard_user(&env, "list_wing_a_owner@dtest.test", "password123");
    let a_state = start_process(&url, &a_owner_jwt, template_id);
    let a_process_id = a_state["process_id"].as_str().unwrap().to_string();

    quick_create_user(&env, &admin_jwt, "list_wing_b_owner@dtest.test", "B", "304");
    let b_owner_jwt = quick_onboard_user(&env, "list_wing_b_owner@dtest.test", "password123");
    let b_state = start_process(&url, &b_owner_jwt, template_id);
    let b_process_id = b_state["process_id"].as_str().unwrap().to_string();

    let a_rep_id = quick_create_user(&env, &admin_jwt, "list_wing_a_rep5@dtest.test", "A", "305");
    let a_rep_jwt = quick_onboard_user(&env, "list_wing_a_rep5@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, a_rep_id, "wing_rep_a");

    let listed = list_processes(&url, &a_rep_jwt);
    let process_ids: Vec<&str> = listed["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["process_id"].as_str().unwrap())
        .collect();

    assert!(
        process_ids.contains(&a_process_id.as_str()),
        "wing A's rep must see wing A's own pending process"
    );
    assert!(
        process_ids.contains(&b_process_id.as_str()),
        "wing A's rep must also see wing B's process, since any wing rep may act on it"
    );
}

/// Wing B's representative CAN revoke/reopen a completed stage on wing A's resident's
/// process — the review/reopen action follows the same cross-wing rule as filling it.
#[tokio::test]
async fn test_wing_representative_can_reopen_a_different_wings_completed_step() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let template_id = create_and_activate_template(&url, &admin_jwt, &wing_representative_draft());

    let _owner_id = quick_create_user(
        &env,
        &admin_jwt,
        "list_wing_a_owner2@dtest.test",
        "A",
        "306",
    );
    let owner_jwt = quick_onboard_user(&env, "list_wing_a_owner2@dtest.test", "password123");

    let a_rep_id = quick_create_user(&env, &admin_jwt, "list_wing_a_rep6@dtest.test", "A", "307");
    let a_rep_jwt = quick_onboard_user(&env, "list_wing_a_rep6@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, a_rep_id, "wing_rep_a");

    let b_rep_id = quick_create_user(&env, &admin_jwt, "list_wing_b_rep2@dtest.test", "B", "308");
    let b_rep_jwt = quick_onboard_user(&env, "list_wing_b_rep2@dtest.test", "password123");
    quick_assign_designation(&env, &admin_jwt, b_rep_id, "wing_rep_b");

    let state = start_process(&url, &owner_jwt, template_id);
    let process_id = state["process_id"].as_str().unwrap();
    let (step_id, field_id) = field_and_step_ids(&state);

    save_step(&url, &a_rep_jwt, process_id, step_id, field_id)
        .expect("wing A's rep completes the step");

    let result = revoke_step(&url, &b_rep_jwt, process_id, step_id)
        .expect("any wing's representative, including wing B's, can reopen the step");
    assert_eq!(result["status"], "success");
}
