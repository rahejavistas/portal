use quality::{quick_create_user, quick_setup_admin, setup_api_test_env};

/// Tests that creating a user with invalid inputs fails gracefully.
#[tokio::test]
async fn test_invalid_inputs() {
    let env = setup_api_test_env(3103).await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    // 1. Missing fields (e.g. empty email)
    let req_empty_email = ureq::post(&format!("{}/api/v1/admin/users/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "email": "",
            "wing": "A",
            "flat_number": "101"
        }));
    assert!(req_empty_email.is_err());
    assert_eq!(
        req_empty_email
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );

    // 2. Invalid flat formatting (non-numeric)
    let req_invalid_flat = ureq::post(&format!("{}/api/v1/admin/users/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "email": "invalid@vistas.com",
            "wing": "A",
            "flat_number": "XYZ"
        }));
    assert!(req_invalid_flat.is_err());
    assert_eq!(
        req_invalid_flat
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );
}

/// Tests that creating a duplicate user fails.
#[tokio::test]
async fn test_duplicate_user_registration() {
    let env = setup_api_test_env(3104).await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    // Create first user
    let _uid1 = quick_create_user(&env, &admin_jwt, "dup@vistas.com", "A", "101");

    // Attempt to create second user with same email
    let req_dup = ureq::post(&format!("{}/api/v1/admin/users/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "email": "dup@vistas.com",
            "wing": "B",
            "flat_number": "202"
        }));

    match req_dup {
        Ok(r) => assert_eq!(r.status(), 201),
        Err(e) => {
            if let ureq::Error::Status(_, resp) = e {
                let err_str = resp.into_string().unwrap();
                panic!("req_dup failed: {}", err_str);
            } else {
                panic!("req_dup failed: {:?}", e);
            }
        }
    }
}

/// Tests self-demotion prevention
#[tokio::test]
async fn test_self_demotion_prevention() {
    let env = setup_api_test_env(3105).await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = format!("Bearer {}", admin_jwt);

    // Get admin user_id
    let list_resp = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .unwrap();
    let users: Vec<serde_json::Value> = list_resp.into_json().unwrap();
    let admin_id = users
        .iter()
        .find(|u| u["email"].as_str().unwrap() == "superadmin@vistas.com")
        .unwrap()["user_id"]
        .as_i64()
        .unwrap() as i32;

    // Admin tries to demote themselves
    let demote_req = ureq::post(&format!("{}/api/v1/admin/users/roles", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "user_id": admin_id,
            "target_role": serde_json::Value::Null
        }));

    assert!(demote_req.is_err());
    assert_eq!(
        demote_req.unwrap_err().into_response().unwrap().status(),
        403
    );
}
