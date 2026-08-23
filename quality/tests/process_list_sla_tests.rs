//! HTTP-level coverage for the SLA/TAT fields on POST /api/v1/process/list — the
//! fields the admin list view sorts by (days_running, effective_due_date, is_overdue).

use quality::{quick_create_user, quick_onboard_user, quick_setup_admin, setup_api_test_env};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

#[tokio::test]
async fn test_process_list_exposes_sla_fields_once_tat_template_is_submitted() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let draft = serde_json::json!({
        "template_id": null,
        "name": "SLA List Test Template",
        "description": null,
        "is_optional": false,
        "is_review_required": false,
        "tat_working_days": 5,
        "substeps": [
            {
                "name": "Only Stage",
                "is_review_required": false,
                "fields": [
                    { "name": "Note", "type": "text", "filled_by": "user" }
                ]
            }
        ]
    });
    let save_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/process-templates/save", url))
            .set("Authorization", &a)
            .send_json(draft)
            .unwrap()
            .into_json()
            .unwrap();
    let version_id = save_resp["version_id"].as_i64().unwrap() as i32;

    let templates: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/process-templates/list", url))
            .set("Authorization", &a)
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
        .set("Authorization", &a)
        .send_json(serde_json::json!({"template_id": template_id, "version_id": version_id}))
        .unwrap();

    let rid = quick_create_user(&env, &admin_jwt, "sla_list_res@test.com", "Z", "701");
    let res_jwt = quick_onboard_user(&env, "sla_list_res@test.com", "password123");
    let res_auth = auth(&res_jwt);

    let start_resp: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": template_id}))
        .unwrap()
        .into_json()
        .unwrap();
    let process_id = start_resp["process_id"].as_str().unwrap().to_string();
    let step_id = start_resp["steps"][0]["template_step_id"].as_i64().unwrap();
    let field_id = start_resp["steps"][0]["fields"][0]["template_field_id"]
        .as_i64()
        .unwrap();

    // Before submission: no SLA tracking should have kicked in yet.
    let list_before: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"personal_only": true}))
        .unwrap()
        .into_json()
        .unwrap();
    let item_before = list_before["processes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["process_id"] == process_id)
        .unwrap();
    assert!(item_before["effective_due_date"].is_null());
    assert!(item_before["days_running"].as_i64().is_some());

    ureq::post(&format!("{}/api/v1/process/save-step", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({
            "process_id": process_id,
            "template_step_id": step_id,
            "fields": [{"template_field_id": field_id, "text_value": "done", "document_id": null}]
        }))
        .unwrap();

    let list_after: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"personal_only": true}))
        .unwrap()
        .into_json()
        .unwrap();
    let item_after = list_after["processes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["process_id"] == process_id)
        .unwrap();
    assert!(
        !item_after["effective_due_date"].is_null(),
        "submitting a TAT-tracked template's only step must set a due date"
    );
    assert_eq!(item_after["is_overdue"], false);
    assert_eq!(item_after["is_sla_paused"], false);

    let _ = rid;
}
