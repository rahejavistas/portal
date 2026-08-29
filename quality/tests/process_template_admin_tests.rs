//! Coverage for the versioned process-template admin CRUD flow: create, edit (new
//! draft version), activate, per-version ID uniqueness, and permission gating on the
//! four admin routes.

use quality::{
    quick_assign_designation, quick_create_designation, quick_create_document, quick_create_user,
    quick_onboard_user, quick_setup_admin, quick_toggle_designation_permission, setup_api_test_env,
    TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn simple_draft(name: &str, template_id: Option<i32>) -> serde_json::Value {
    serde_json::json!({
        "template_id": template_id,
        "name": name,
        "description": null,
        "is_optional": false,
        "is_review_required": false,
        "substeps": [
            {
                "name": "Stage 1",
                "is_review_required": true,
                "substeps": [
                    {
                        "name": "Substep 1",
                        "fields": [
                            { "name": "Field A", "type": "text", "filled_by": "user" },
                            { "name": "Field B", "type": "document", "filled_by": "user" }
                        ]
                    }
                ]
            }
        ]
    })
}

fn save(url: &str, jwt: &str, draft: &serde_json::Value) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
        .set("Authorization", &auth(jwt))
        .send_json(draft.clone());
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn get_version(url: &str, jwt: &str, version_id: i32) -> Result<serde_json::Value, u16> {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/get-version",
        url
    ))
    .set("Authorization", &auth(jwt))
    .send_json(serde_json::json!({ "version_id": version_id }));
    match resp {
        Ok(r) => Ok(r.into_json().unwrap()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn activate(url: &str, jwt: &str, template_id: i32, version_id: i32) -> Result<(), u16> {
    let resp = ureq::post(&format!("{}/api/v1/admin/process-templates/activate", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({ "template_id": template_id, "version_id": version_id }));
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn deactivate(url: &str, jwt: &str, template_id: i32) -> Result<(), u16> {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/deactivate",
        url
    ))
    .set("Authorization", &auth(jwt))
    .send_json(serde_json::json!({ "template_id": template_id }));
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(e) => panic!("transport error: {:?}", e),
    }
}

fn list_templates(url: &str, jwt: &str) -> Vec<serde_json::Value> {
    ureq::post(&format!("{}/api/v1/admin/process-templates/list", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({}))
        .expect("list failed")
        .into_json()
        .unwrap()
}

fn available_templates(url: &str, jwt: &str) -> Vec<serde_json::Value> {
    ureq::post(&format!("{}/api/v1/process-templates/available", url))
        .set("Authorization", &auth(jwt))
        .send_json(serde_json::json!({}))
        .expect("available failed")
        .into_json()
        .unwrap()
}

/// Collects every step_id and field_id appearing in a `ProcessTemplate` JSON tree
/// (root + substeps + fields, recursively).
fn collect_ids(tree: &serde_json::Value, step_ids: &mut Vec<i64>, field_ids: &mut Vec<i64>) {
    if let Some(sid) = tree.get("step_id").and_then(|v| v.as_i64()) {
        step_ids.push(sid);
    }
    if let Some(fields) = tree.get("fields").and_then(|v| v.as_array()) {
        for f in fields {
            if let Some(fid) = f.get("field_id").and_then(|v| v.as_i64()) {
                field_ids.push(fid);
            }
        }
    }
    if let Some(substeps) = tree.get("substeps").and_then(|v| v.as_array()) {
        for s in substeps {
            collect_ids(s, step_ids, field_ids);
        }
    }
}

fn seeded_manager(env: &TestEnv, admin_jwt: &str, tag: &str, flat_number: &str) -> String {
    let designation_code = format!("tmpl_{}", tag);
    quick_create_designation(env, admin_jwt, &designation_code, &designation_code);
    quick_toggle_designation_permission(
        env,
        admin_jwt,
        &designation_code,
        "process.templates.manage",
        true,
    );
    let email = format!("{}@tmpladmin.test", tag);
    let uid = quick_create_user(env, admin_jwt, &email, "Z", flat_number);
    let jwt = quick_onboard_user(env, &email, "password123");
    quick_assign_designation(env, admin_jwt, uid, &designation_code);
    jwt
}

/// A brand-new template is created as a draft: it gets a fresh template_id and its
/// first version, but `active_version_id` stays NULL until an explicit Activate —
/// so it must not show up in the resident-facing "available" list yet.
#[tokio::test]
async fn test_create_new_template_is_draft_not_active() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "create", "901");

    let resp = save(&url, &manager_jwt, &simple_draft("Draft Template", None)).unwrap();
    let version_id = resp["version_id"].as_i64().unwrap() as i32;

    let templates = list_templates(&url, &manager_jwt);
    let entry = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(version_id as i64))
        })
        .expect("newly created template not found in list");
    assert!(
        entry["active_version_id"].is_null(),
        "a new template must not auto-activate"
    );
    assert_eq!(entry["versions"].as_array().unwrap().len(), 1);
    assert_eq!(entry["versions"][0]["version_number"].as_i64(), Some(1));

    let template_id = entry["template_id"].as_i64().unwrap() as i32;
    let available = available_templates(&url, &manager_jwt);
    assert!(
        !available
            .iter()
            .any(|t| t["template_id"].as_i64() == Some(template_id as i64)),
        "an unactivated template must not be startable"
    );
}

/// Activating a version makes the template startable, and pins a newly-started
/// process to exactly that version_id.
#[tokio::test]
async fn test_activate_makes_template_startable_and_pins_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "activate", "902");

    let resp = save(
        &url,
        &manager_jwt,
        &simple_draft("Activatable Template", None),
    )
    .unwrap();
    let version_id = resp["version_id"].as_i64().unwrap() as i32;
    let templates = list_templates(&url, &manager_jwt);
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

    activate(&url, &manager_jwt, template_id, version_id).expect("activate should succeed");

    let available = available_templates(&url, &manager_jwt);
    assert!(available
        .iter()
        .any(|t| t["template_id"].as_i64() == Some(template_id as i64)));

    quick_create_user(&env, &admin_jwt, "resident_pin@tmpladmin.test", "A", "903");
    let resident_jwt = quick_onboard_user(&env, "resident_pin@tmpladmin.test", "password123");
    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&resident_jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        state["template_version_id"].as_i64(),
        Some(version_id as i64)
    );
}

/// Editing an existing template appends a new draft version — it does NOT move
/// `active_version_id`, and the previous version's content is left untouched.
#[tokio::test]
async fn test_edit_appends_new_version_without_auto_activating() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "edit", "904");

    let v1_resp = save(&url, &manager_jwt, &simple_draft("Editable Template", None)).unwrap();
    let v1_id = v1_resp["version_id"].as_i64().unwrap() as i32;
    let templates = list_templates(&url, &manager_jwt);
    let template_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(v1_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;
    activate(&url, &manager_jwt, template_id, v1_id).unwrap();

    let mut edited_draft = simple_draft("Editable Template (v2 edit)", Some(template_id));
    edited_draft["substeps"][0]["substeps"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "name": "Field C", "type": "text", "filled_by": "admin" }));
    let v2_resp = save(&url, &manager_jwt, &edited_draft).unwrap();
    let v2_id = v2_resp["version_id"].as_i64().unwrap() as i32;
    assert_ne!(v1_id, v2_id);

    let templates = list_templates(&url, &manager_jwt);
    let entry = templates
        .iter()
        .find(|t| t["template_id"].as_i64() == Some(template_id as i64))
        .unwrap();
    assert_eq!(
        entry["active_version_id"].as_i64(),
        Some(v1_id as i64),
        "edit must not auto-activate"
    );
    let versions = entry["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert!(versions
        .iter()
        .any(|v| v["version_number"].as_i64() == Some(2)));

    let v1_tree = get_version(&url, &manager_jwt, v1_id).unwrap();
    assert_eq!(v1_tree["name"], "Editable Template");
    let mut step_ids = vec![];
    let mut field_ids = vec![];
    collect_ids(&v1_tree, &mut step_ids, &mut field_ids);
    assert_eq!(field_ids.len(), 2, "v1 must be untouched by the v2 edit");
}

/// A process already in progress on an old version must stay pinned to it even after
/// a newer version of the same template is created and activated.
#[tokio::test]
async fn test_in_progress_process_stays_pinned_after_new_version_activated() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "pin", "905");

    let v1_resp = save(&url, &manager_jwt, &simple_draft("Pinned Template", None)).unwrap();
    let v1_id = v1_resp["version_id"].as_i64().unwrap() as i32;
    let templates = list_templates(&url, &manager_jwt);
    let template_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(v1_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;
    activate(&url, &manager_jwt, template_id, v1_id).unwrap();

    quick_create_user(&env, &admin_jwt, "resident_stay@tmpladmin.test", "A", "906");
    let resident_jwt = quick_onboard_user(&env, "resident_stay@tmpladmin.test", "password123");
    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&resident_jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(state["template_version_id"].as_i64(), Some(v1_id as i64));
    let process_id = state["process_id"].as_str().unwrap().to_string();

    let v2_resp = save(
        &url,
        &manager_jwt,
        &simple_draft("Pinned Template (v2)", Some(template_id)),
    )
    .unwrap();
    let v2_id = v2_resp["version_id"].as_i64().unwrap() as i32;
    activate(&url, &manager_jwt, template_id, v2_id).unwrap();

    let refetched: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&resident_jwt))
        .send_json(serde_json::json!({ "process_id": process_id }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        refetched["template_version_id"].as_i64(),
        Some(v1_id as i64),
        "an in-progress process must stay pinned to the version it started on"
    );

    quick_create_user(
        &env,
        &admin_jwt,
        "resident_fresh@tmpladmin.test",
        "A",
        "907",
    );
    let fresh_jwt = quick_onboard_user(&env, "resident_fresh@tmpladmin.test", "password123");
    let fresh_state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&fresh_jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        fresh_state["template_version_id"].as_i64(),
        Some(v2_id as i64),
        "a brand-new process must start against the currently active version"
    );
}

/// step_id/field_id only need to be unique WITHIN one version's `definition` — the
/// same numbers are expected (and fine) to repeat across different versions of the
/// same template, and across entirely different templates too, since every step/field
/// is only ever read together with the process_id that pins it to one exact version.
#[tokio::test]
async fn test_ids_unique_within_a_version_but_reused_freely_elsewhere() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "idalloc", "908");

    let v1_resp = save(&url, &manager_jwt, &simple_draft("ID Template", None)).unwrap();
    let v1_id = v1_resp["version_id"].as_i64().unwrap() as i32;
    let v1_tree = get_version(&url, &manager_jwt, v1_id).unwrap();
    let mut v1_steps = vec![];
    let mut v1_fields = vec![];
    collect_ids(&v1_tree, &mut v1_steps, &mut v1_fields);

    let mut all_v1_ids = v1_steps.clone();
    all_v1_ids.extend(v1_fields.clone());
    let unique: std::collections::HashSet<_> = all_v1_ids.iter().collect();
    assert_eq!(
        unique.len(),
        all_v1_ids.len(),
        "IDs within one version must be unique"
    );

    let templates = list_templates(&url, &manager_jwt);
    let template_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(v1_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;

    // A second version of the SAME template, and a totally separate template, both
    // legitimately reuse the exact same step_id/field_id numbers as v1 — no collision
    // avoidance across versions/templates is expected or needed.
    let v2_resp = save(
        &url,
        &manager_jwt,
        &simple_draft("ID Template (v2)", Some(template_id)),
    )
    .unwrap();
    let v2_id = v2_resp["version_id"].as_i64().unwrap() as i32;
    let v2_tree = get_version(&url, &manager_jwt, v2_id).unwrap();
    let mut v2_steps = vec![];
    let mut v2_fields = vec![];
    collect_ids(&v2_tree, &mut v2_steps, &mut v2_fields);
    let mut all_v2_ids = v2_steps;
    all_v2_ids.extend(v2_fields);
    assert_eq!(
        all_v1_ids, all_v2_ids,
        "versions of the same template reuse the same IDs"
    );

    let t2_resp = save(&url, &manager_jwt, &simple_draft("Other Template", None)).unwrap();
    let t2_version_id = t2_resp["version_id"].as_i64().unwrap() as i32;
    let t2_tree = get_version(&url, &manager_jwt, t2_version_id).unwrap();
    let mut t2_steps = vec![];
    let mut t2_fields = vec![];
    collect_ids(&t2_tree, &mut t2_steps, &mut t2_fields);
    let mut t2_ids = t2_steps;
    t2_ids.extend(t2_fields);
    assert_eq!(
        all_v1_ids, t2_ids,
        "a totally different template reuses the same IDs too"
    );
}

/// Activating a version_id that doesn't belong to the given template_id must be
/// rejected, not silently activate the wrong template.
#[tokio::test]
async fn test_activate_rejects_version_from_different_template() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "mismatch", "910");

    let t1_resp = save(
        &url,
        &manager_jwt,
        &simple_draft("Mismatch Template One", None),
    )
    .unwrap();
    let t1_version_id = t1_resp["version_id"].as_i64().unwrap() as i32;
    let templates = list_templates(&url, &manager_jwt);
    let t1_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(t1_version_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;

    let t2_resp = save(
        &url,
        &manager_jwt,
        &simple_draft("Mismatch Template Two", None),
    )
    .unwrap();
    let t2_version_id = t2_resp["version_id"].as_i64().unwrap() as i32;

    // t2_version_id does not belong to t1_id — must be rejected.
    let result = activate(&url, &manager_jwt, t1_id, t2_version_id);
    assert!(
        result.is_err(),
        "activating a mismatched (template_id, version_id) pair must fail"
    );
}

/// Only a designation holding `process.templates.manage` can reach any of the four
/// admin routes; everyone else gets 403. `available` needs no special permission —
/// any authenticated user can see which templates they're allowed to start.
#[tokio::test]
async fn test_permission_gating_for_template_routes() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "gate_ok", "911");

    let v1_resp = save(&url, &manager_jwt, &simple_draft("Gated Template", None)).unwrap();
    let v1_id = v1_resp["version_id"].as_i64().unwrap() as i32;
    let templates = list_templates(&url, &manager_jwt);
    let template_id = templates
        .iter()
        .find(|t| {
            t["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["version_id"].as_i64() == Some(v1_id as i64))
        })
        .unwrap()["template_id"]
        .as_i64()
        .unwrap() as i32;

    quick_create_user(&env, &admin_jwt, "no_perm@tmpladmin.test", "A", "912");
    let no_perm_jwt = quick_onboard_user(&env, "no_perm@tmpladmin.test", "password123");

    let list_resp = ureq::post(&format!("{}/api/v1/admin/process-templates/list", url))
        .set("Authorization", &auth(&no_perm_jwt))
        .send_json(serde_json::json!({}));
    assert_eq!(
        list_resp.unwrap_err().into_response().unwrap().status(),
        403
    );

    assert_eq!(get_version(&url, &no_perm_jwt, v1_id).unwrap_err(), 403);
    assert_eq!(
        save(&url, &no_perm_jwt, &simple_draft("Should Fail", None)).unwrap_err(),
        403
    );
    assert_eq!(
        activate(&url, &no_perm_jwt, template_id, v1_id).unwrap_err(),
        403
    );
    assert_eq!(
        deactivate(&url, &no_perm_jwt, template_id).unwrap_err(),
        403
    );
    assert_eq!(
        ureq::post(&format!(
            "{}/api/v1/admin/process-templates/tat-report",
            url
        ))
        .set("Authorization", &auth(&no_perm_jwt))
        .send_json(serde_json::json!({}))
        .unwrap_err()
        .into_response()
        .unwrap()
        .status(),
        403
    );
    assert_eq!(
        ureq::post(&format!(
            "{}/api/v1/admin/process-templates/holidays/list",
            url
        ))
        .set("Authorization", &auth(&no_perm_jwt))
        .send_json(serde_json::json!({}))
        .unwrap_err()
        .into_response()
        .unwrap()
        .status(),
        403
    );
    assert_eq!(
        ureq::post(&format!(
            "{}/api/v1/admin/process-templates/holidays/add",
            url
        ))
        .set("Authorization", &auth(&no_perm_jwt))
        .send_json(serde_json::json!({"holiday_date": "2026-01-26", "description": "Republic Day"}))
        .unwrap_err()
        .into_response()
        .unwrap()
        .status(),
        403
    );
    assert_eq!(
        ureq::post(&format!(
            "{}/api/v1/admin/process-templates/holidays/remove",
            url
        ))
        .set("Authorization", &auth(&no_perm_jwt))
        .send_json(serde_json::json!({"holiday_date": "2026-01-26"}))
        .unwrap_err()
        .into_response()
        .unwrap()
        .status(),
        403
    );

    let add_resp: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/holidays/add",
        url
    ))
    .set("Authorization", &auth(&manager_jwt))
    .send_json(serde_json::json!({"holiday_date": "2026-01-26", "description": "Republic Day"}))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(add_resp["status"], "ok");

    let list_resp: Vec<serde_json::Value> = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/holidays/list",
        url
    ))
    .set("Authorization", &auth(&manager_jwt))
    .send_json(serde_json::json!({}))
    .unwrap()
    .into_json()
    .unwrap();
    assert!(list_resp
        .iter()
        .any(|h| h["holiday_date"] == "2026-01-26" && h["description"] == "Republic Day"));

    let remove_resp = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/holidays/remove",
        url
    ))
    .set("Authorization", &auth(&manager_jwt))
    .send_json(serde_json::json!({"holiday_date": "2026-01-26"}));
    assert!(remove_resp.is_ok());

    let tat_report_resp = ureq::post(&format!(
        "{}/api/v1/admin/process-templates/tat-report",
        url
    ))
    .set("Authorization", &auth(&manager_jwt))
    .send_json(serde_json::json!({}));
    assert!(tat_report_resp.is_ok());

    // No special permission needed for the resident-facing "available" endpoint.
    let available_resp = ureq::post(&format!("{}/api/v1/process-templates/available", url))
        .set("Authorization", &auth(&no_perm_jwt))
        .send_json(serde_json::json!({}));
    assert!(
        available_resp.is_ok(),
        "available should be reachable by any authenticated user"
    );
}

/// Deactivating a template clears active_version_id so it drops out of the
/// resident-facing "available" list, but an already-started process stays pinned to
/// its own version and keeps working normally.
#[tokio::test]
async fn test_deactivate_hides_from_available_but_in_progress_process_keeps_working() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "deactivate", "913");

    let resp = save(
        &url,
        &manager_jwt,
        &simple_draft("Deactivate Me Template", None),
    )
    .unwrap();
    let version_id = resp["version_id"].as_i64().unwrap() as i32;
    let templates = list_templates(&url, &manager_jwt);
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

    activate(&url, &manager_jwt, template_id, version_id).expect("activate should succeed");
    assert!(available_templates(&url, &manager_jwt)
        .iter()
        .any(|t| t["template_id"].as_i64() == Some(template_id as i64)));

    quick_create_user(
        &env,
        &admin_jwt,
        "deactivate_res@tmpladmin.test",
        "D",
        "914",
    );
    let res_jwt = quick_onboard_user(&env, "deactivate_res@tmpladmin.test", "password123");
    let start_resp: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&res_jwt))
        .send_json(serde_json::json!({"template_id": template_id}))
        .unwrap()
        .into_json()
        .unwrap();
    let process_id = start_resp["process_id"].as_str().unwrap().to_string();

    deactivate(&url, &manager_jwt, template_id).expect("deactivate should succeed");

    assert!(
        !available_templates(&url, &manager_jwt)
            .iter()
            .any(|t| t["template_id"].as_i64() == Some(template_id as i64)),
        "deactivated template must drop out of the resident-facing available list"
    );
    let templates_after = list_templates(&url, &manager_jwt);
    let entry_after = templates_after
        .iter()
        .find(|t| t["template_id"].as_i64() == Some(template_id as i64))
        .unwrap();
    assert!(entry_after["active_version_id"].is_null());

    let reload_resp = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&res_jwt))
        .send_json(serde_json::json!({"process_id": process_id}));
    assert!(
        reload_resp.is_ok(),
        "an in-progress process must keep working after its template is deactivated"
    );
}

/// A document field's `example_document_id` round-trips through save/get-version, and
/// shows up (with its resolved title) in the resident-facing process state — the download
/// affordance the resident sees is driven entirely by this template-level reference.
#[tokio::test]
async fn test_document_field_carries_example_document_through_to_resident_state() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "example", "904");

    let example_doc_id =
        quick_create_document(&env, &admin_jwt, "Sample Undertaking", "PUBLIC", None);

    let mut draft = simple_draft("Template With Example Doc", None);
    draft["substeps"][0]["substeps"][0]["fields"][1]["example_document_id"] =
        serde_json::Value::String(example_doc_id.clone());

    let resp = save(&url, &manager_jwt, &draft).unwrap();
    let version_id = resp["version_id"].as_i64().unwrap() as i32;

    let fetched = get_version(&url, &manager_jwt, version_id).unwrap();
    let doc_field = &fetched["substeps"][0]["substeps"][0]["fields"][1];
    assert_eq!(
        doc_field["example_document_id"].as_str(),
        Some(example_doc_id.as_str()),
        "example_document_id must round-trip through save/get-version"
    );

    let templates = list_templates(&url, &manager_jwt);
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
    activate(&url, &manager_jwt, template_id, version_id).expect("activate should succeed");

    quick_create_user(&env, &admin_jwt, "resident_ex@tmpladmin.test", "A", "905");
    let resident_jwt = quick_onboard_user(&env, "resident_ex@tmpladmin.test", "password123");
    let state: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&resident_jwt))
        .send_json(serde_json::json!({ "template_id": template_id }))
        .unwrap()
        .into_json()
        .unwrap();

    let field = &state["steps"][0]["substeps"][0]["fields"][1];
    assert_eq!(field["name"].as_str(), Some("Field B"));
    assert_eq!(
        field["example_document_id"].as_str(),
        Some(example_doc_id.as_str())
    );
    assert_eq!(
        field["example_document_title"].as_str(),
        Some("Sample Undertaking")
    );
}

/// A non-PUBLIC document can't be set as an example — the picker only offers PUBLIC
/// documents, but the save endpoint must reject a direct API call too, since the
/// resolved title is disclosed to every resident who views that step.
#[tokio::test]
async fn test_save_rejects_non_public_example_document() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manager_jwt = seeded_manager(&env, &admin_jwt, "examplenonpub", "906");

    let private_doc_id = quick_create_document(
        &env,
        &admin_jwt,
        "Someone's Private Bill",
        "RESIDENT_SPECIFIC",
        Some(("Z", "906")),
    );

    let mut draft = simple_draft("Template With Non-Public Example Doc", None);
    draft["substeps"][0]["substeps"][0]["fields"][1]["example_document_id"] =
        serde_json::Value::String(private_doc_id);

    let status = save(&url, &manager_jwt, &draft).unwrap_err();
    assert_eq!(status, 400);
}
