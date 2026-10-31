//! Flat creation and the fire-flat safety flag: bulk-create idempotency, wing
//! normalization, and toggling a flat's fire-flat status.

use quality::{quick_create_user, quick_setup_admin, setup_api_test_env};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

#[tokio::test]
async fn test_create_flat_is_idempotent_on_duplicate_wing_and_number() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    for _ in 0..2 {
        let resp = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"wing": "Q", "flat_number": "101"}))
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let matching = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["wing"] == "Q" && f["flat_number"] == "0101")
        .count();
    assert_eq!(matching, 1, "duplicate create must not create a second row");
}

#[tokio::test]
async fn test_bulk_create_flats_skips_already_existing_numbers() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "R", "flat_number": "201"}))
        .unwrap();

    let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/bulk-create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "R", "flat_numbers": ["201", "202", "203"]}))
        .unwrap()
        .into_json()
        .unwrap();

    assert_eq!(
        resp["created"], 2,
        "only the two genuinely new flats should count as created"
    );
}

#[tokio::test]
async fn test_create_flat_normalizes_wing_case_so_duplicates_still_collide() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "u", "flat_number": "101"}))
        .unwrap();
    ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "U", "flat_number": "101"}))
        .unwrap();

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let matching = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["wing"] == "U" && f["flat_number"] == "0101")
        .count();
    assert_eq!(
        matching, 1,
        "'u' and 'U' must normalize to the same wing and collide, not create two rows"
    );
}

#[tokio::test]
async fn test_set_flat_fire_flag_toggles_and_persists() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let created: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "V", "flat_number": "301"}))
        .unwrap()
        .into_json()
        .unwrap();
    let flat_id = created["flat_id"].as_i64().unwrap();
    assert_eq!(created["is_fire_flat"], false);

    ureq::post(&format!("{}/api/v1/admin/flats/set-fire-flag", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": flat_id, "is_fire_flat": true}))
        .unwrap();

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let flat = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["flat_id"] == flat_id)
        .unwrap();
    assert_eq!(flat["is_fire_flat"], true);

    ureq::post(&format!("{}/api/v1/admin/flats/set-fire-flag", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": flat_id, "is_fire_flat": false}))
        .unwrap();

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let flat = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["flat_id"] == flat_id)
        .unwrap();
    assert_eq!(flat["is_fire_flat"], false);
}

#[tokio::test]
async fn test_set_flat_fire_flag_rejects_unknown_flat_id() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/set-fire-flag", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": 999999, "is_fire_flat": true}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);
}

#[tokio::test]
async fn test_delete_flat_removes_vacant_flat() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let created: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "W", "flat_number": "401"}))
        .unwrap()
        .into_json()
        .unwrap();
    let flat_id = created["flat_id"].as_i64().unwrap();

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/delete", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": flat_id}));
    assert!(resp.is_ok());

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(!list
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["flat_id"] == flat_id));
}

#[tokio::test]
async fn test_delete_flat_rejects_occupied_flat() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let rid = quick_create_user(&env, &admin_jwt, "occupied_delete@test.com", "W", "402");

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let flat_id = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["wing"] == "W" && f["flat_number"] == "0402")
        .unwrap()["flat_id"]
        .as_i64()
        .unwrap();

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/delete", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": flat_id}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);

    let list_after: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        list_after
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["flat_id"] == flat_id),
        "occupied flat must not be deleted"
    );
    let _ = rid;
}

#[tokio::test]
async fn test_delete_flat_rejects_unknown_flat_id() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/delete", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": 999999}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);
}

#[tokio::test]
async fn test_create_flat_rejects_non_positive_area_sqft() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "X", "flat_number": "501", "area_sqft": 0}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "X", "flat_number": "502", "area_sqft": -50}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);
}

#[tokio::test]
async fn test_create_flat_rejects_numeric_wing() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "3", "flat_number": "101"}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);
}

#[tokio::test]
async fn test_bulk_create_flats_rejects_non_numeric_flat_number() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/bulk-create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "Y", "flat_numbers": ["101", "abc"]}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        !list.as_array().unwrap().iter().any(|f| f["wing"] == "Y"),
        "an all-or-nothing bulk-create must not partially insert on a bad entry"
    );
}

#[tokio::test]
async fn test_delete_flat_rejects_when_financial_records_reference_it() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    let created: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/create", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"wing": "Z", "flat_number": "601"}))
        .unwrap()
        .into_json()
        .unwrap();
    let flat_id = created["flat_id"].as_i64().unwrap();

    let fund_id: i32 = sqlx::query_scalar("SELECT fund_id FROM funds WHERE code = 'OPERATING'")
        .fetch_one(&env.db_pool)
        .await
        .unwrap();
    let assign_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/funds/assign-flat",
        url
    ))
    .set("Authorization", &a)
    .send_json(serde_json::json!({"flat_id": flat_id, "fund_id": fund_id}));
    assert!(assign_resp.is_ok(), "fund assignment should succeed");

    let resp = ureq::post(&format!("{}/api/v1/admin/flats/delete", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"flat_id": flat_id}));
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|f| f["flat_id"] == flat_id),
        "flat with financial records must not be deleted"
    );
}

fn upload_members(url: &str, auth_header: &str, file: &[u8]) -> serde_json::Value {
    let boundary = "----membersupload";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"members.xlsx\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    ureq::post(&format!("{}/api/v1/admin/users/upload", url))
        .set("Authorization", auth_header)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body)
        .unwrap()
        .into_json()
        .unwrap()
}

#[tokio::test]
async fn test_bulk_user_upload_registers_wings_and_flats_and_is_idempotent() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);
    let fixture = include_bytes!("../fixtures/member_data_sample.xlsx");

    let first = upload_members(&url, &a, fixture);
    assert_eq!(
        first["successfully_onboarded"], 4,
        "three EmailIDs plus the one CCMail, which becomes the second owner of Z-0002"
    );
    assert_eq!(first["flats_created"], 3);

    let second = upload_members(&url, &a, fixture);
    assert_eq!(
        second["flats_created"], 0,
        "re-uploading the same file must not create duplicate flats"
    );
    assert_eq!(
        second["successfully_onboarded"], 4,
        "re-adding the same owners is a no-op, not a 'flat is full' failure"
    );
    assert_eq!(second["failed_users"].as_array().unwrap().len(), 1);

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/admin/flats/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let mut numbers: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["wing"] == "Z")
        .map(|f| f["flat_number"].as_str().unwrap())
        .collect();
    numbers.sort_unstable();
    assert_eq!(numbers, ["0001", "0002", "0003"]);

    let owners_of = |flat_number: &str| -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .find(|f| f["wing"] == "Z" && f["flat_number"] == flat_number)
            .map(|f| {
                f["owner_emails"]
                    .as_array()
                    .unwrap_or_else(|| panic!("no owner_emails array in {f}"))
                    .iter()
                    .map(|e| e.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap()
    };
    assert_eq!(
        owners_of("0002"),
        ["cc.two@example.test", "resident.two@example.test"],
        "EmailID and CCMail must both be registered as owners of the unit"
    );
    assert_eq!(owners_of("0001"), ["resident.one@example.test"]);
    assert_eq!(owners_of("0003"), ["resident.three@example.test"]);
}
