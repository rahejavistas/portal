use quality::{
    quick_create_document, quick_create_user, quick_onboard_user, quick_setup_admin,
    setup_api_test_env,
};

/// Tests that API endpoints properly reject requests with missing or invalid tokens.
#[tokio::test]
async fn test_unauthorized_access() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    // 1. Missing Authorization header
    let resp_missing =
        ureq::post(&format!("{}/api/v1/profile", url)).send_json(serde_json::json!({}));

    assert!(resp_missing.is_err());
    assert_eq!(
        resp_missing.unwrap_err().into_response().unwrap().status(),
        401
    );

    // 2. Invalid Authorization header
    let resp_invalid = ureq::post(&format!("{}/api/v1/profile", url))
        .set("Authorization", "Bearer invalid.token.string")
        .send_json(serde_json::json!({}));

    assert!(resp_invalid.is_err());
    assert_eq!(
        resp_invalid.unwrap_err().into_response().unwrap().status(),
        401
    );

    // 3. Documents endpoint missing token
    let resp_docs =
        ureq::post(&format!("{}/api/v1/documents", url)).send_json(serde_json::json!({}));

    assert!(resp_docs.is_err());
    assert_eq!(
        resp_docs.unwrap_err().into_response().unwrap().status(),
        401
    );
}

/// Tests that a resident cannot access admin endpoints or private documents of other flats.
#[tokio::test]
async fn test_malicious_user_access() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);

    // Setup two residents
    let _uid1 = quick_create_user(&env, &admin_jwt, "res1_sec@vistas.com", "A", "101");
    let _uid2 = quick_create_user(&env, &admin_jwt, "res2_sec@vistas.com", "B", "202");

    let res1_jwt = quick_onboard_user(&env, "res1_sec@vistas.com", "password123");
    let res1_auth = format!("Bearer {}", res1_jwt);

    // 1. Resident tries to access Admin endpoint
    let admin_req = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &res1_auth)
        .send_json(serde_json::json!({}));

    assert!(admin_req.is_err());
    assert_eq!(
        admin_req.unwrap_err().into_response().unwrap().status(),
        403
    );

    // 2. Create a private document for Resident 2
    let doc2_id = quick_create_document(
        &env,
        &admin_jwt,
        "Secret Deed B-202",
        "RESIDENT_SPECIFIC",
        Some(("B", "202")),
    );

    // 3. Resident 1 tries to access Resident 2's document directly by ID
    let doc_req = ureq::post(&format!("{}/api/v1/documents/{}", url, doc2_id))
        .set("Authorization", &res1_auth)
        .send_json(serde_json::json!({}));

    assert!(doc_req.is_err());
    assert_eq!(doc_req.unwrap_err().into_response().unwrap().status(), 403);
}

/// Tests that an account is locked after 5 failed login attempts and requires a password reset.
#[tokio::test]
async fn test_account_lockout() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let email = "lockout_user@vistas.com";
    let _ = quick_create_user(&env, &admin_jwt, email, "C", "303");
    let _ = quick_onboard_user(&env, email, "password123");

    // 5 consecutive failed logins
    for _ in 0..5 {
        let bad_login = ureq::post(&format!("{}/api/v1/login", url)).send_json(serde_json::json!({
            "email": email,
            "password": "wrongpassword"
        }));
        assert!(bad_login.is_err());
        assert_eq!(
            bad_login.unwrap_err().into_response().unwrap().status(),
            401
        );
    }

    // Must be rejected identically to a wrong password, even with the right one.
    let locked_login = ureq::post(&format!("{}/api/v1/login", url)).send_json(serde_json::json!({
        "email": email,
        "password": "password123"
    }));
    assert!(locked_login.is_err());
    let locked_err = locked_login.unwrap_err().into_response().unwrap();
    assert_eq!(locked_err.status(), 401);

    let err_json: serde_json::Value = locked_err.into_json().unwrap();
    assert_eq!(
        err_json["message"].as_str().unwrap(),
        "Invalid email or password"
    );
}

/// Real vs. fake email get an identical response, and get rate-limited the same way.
#[tokio::test]
async fn test_forgot_password_is_rate_limited_per_email_without_leaking_existence() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let real_email = "forgot_pw_real@vistas.com";
    // Not quick_onboard_user — it calls forgot-password itself, spending the quota.
    let _ = quick_create_user(&env, &admin_jwt, real_email, "C", "304");
    let fake_email = "forgot_pw_never_registered@vistas.com";

    let request = |email: &str| -> (u16, serde_json::Value) {
        match ureq::post(&format!("{}/api/v1/forgot-password", url))
            .send_json(serde_json::json!({ "email": email }))
        {
            Ok(r) => (r.status(), r.into_json().unwrap()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_json().unwrap_or_default()),
            Err(e) => panic!("transport error: {:?}", e),
        }
    };

    let (real_status, real_body) = request(real_email);
    let (fake_status, fake_body) = request(fake_email);
    assert_eq!(real_status, 200);
    assert_eq!(fake_status, 200);
    assert_eq!(
        real_body, fake_body,
        "a registered and an unregistered email must get an identical response"
    );

    // One request already made above; two more fit in the burst of 3, then blocked.
    assert_eq!(request(real_email).0, 200);
    assert_eq!(request(real_email).0, 200);
    let (limited_status, _) = request(real_email);
    assert_eq!(
        limited_status, 429,
        "repeated requests for the same address must eventually be rate-limited"
    );

    // A different address is unaffected.
    assert_eq!(request("forgot_pw_unrelated@vistas.com").0, 200);
}
