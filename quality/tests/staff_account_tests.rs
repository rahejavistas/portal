use quality::{quick_onboard_user, quick_setup_admin, setup_api_test_env};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

#[tokio::test]
async fn test_create_staff_grants_default_permissions() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/users/create-staff", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"email": "office_staff@test.com"}))
        .expect("create-staff should succeed")
        .into_json()
        .unwrap();

    assert_eq!(resp["designations"], serde_json::json!(["staff"]));
    assert_eq!(resp["can_manage_users"], serde_json::json!(true));
    assert_eq!(resp["can_view_users"], serde_json::json!(true));
    assert_eq!(resp["can_view_all_processes"], serde_json::json!(true));
    assert_eq!(resp["can_review_process"], serde_json::json!(true));
    assert_eq!(resp["can_manage_documents"], serde_json::json!(false));
    assert_eq!(resp["can_manage_roles"], serde_json::json!(false));
    assert_eq!(resp["can_manage_designations"], serde_json::json!(false));
}

#[tokio::test]
async fn test_staff_account_can_log_in_and_use_default_permissions() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    ureq::post(&format!("{}/api/v1/admin/users/create-staff", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"email": "staff2@test.com"}))
        .expect("create-staff should succeed");

    let staff_jwt = quick_onboard_user(&env, "staff2@test.com", "password123");
    let a = auth(&staff_jwt);

    assert!(
        ureq::post(&format!("{}/api/v1/admin/users/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({}))
            .is_ok(),
        "staff should be able to list residents (users.view/manage)"
    );

    assert!(
        ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({}))
            .is_err(),
        "staff must not have designations.manage by default"
    );

    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        docs.is_empty(),
        "staff must not have document-category access by default"
    );
}

#[tokio::test]
async fn test_create_staff_requires_users_manage() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"code": "no_users_manage", "label": "No Users Manage"}))
        .unwrap();
    ureq::post(&format!(
        "{}/api/v1/admin/roles/designations/permissions/toggle",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({
        "designation_code": "no_users_manage",
        "permission_code": "process.reviewer",
        "enabled": true
    }))
    .unwrap();

    let rid: i32 = {
        let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/users/create", url))
            .set("Authorization", &auth(&admin_jwt))
            .send_json(serde_json::json!({"email": "scoped_staff@test.com", "wing": "S", "flat_number": "1"}))
            .unwrap()
            .into_json()
            .unwrap();
        resp["user_id"].as_i64().unwrap() as i32
    };
    ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"user_id": rid, "designation_code": "no_users_manage"}))
        .unwrap();
    let scoped_jwt = quick_onboard_user(&env, "scoped_staff@test.com", "password123");

    let err = ureq::post(&format!("{}/api/v1/admin/users/create-staff", url))
        .set("Authorization", &auth(&scoped_jwt))
        .send_json(serde_json::json!({"email": "unauthorized_staff@test.com"}));
    assert!(err.is_err(), "create-staff must require users.manage");
}
