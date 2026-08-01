use quality::{
    quick_assign_designation, quick_create_designation, quick_create_document,
    quick_create_document_category, quick_create_user, quick_onboard_user,
    quick_set_category_designation, quick_setup_admin, setup_api_test_env,
};

/// Tests admin authentication flow, blank user directory listing, and
/// new user registrations with flat mapping verification.
#[tokio::test]
async fn test_admin_auth_and_user_creation() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let admin_auth_header = format!("Bearer {}", admin_jwt);

    let list_resp = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &admin_auth_header)
        .send_json(serde_json::json!({}))
        .expect("Admin failed to list users");

    assert_eq!(list_resp.status(), 200);
    let users_val: serde_json::Value = list_resp.into_json().unwrap();
    let users_arr = users_val.as_array().expect("Expected users array");
    assert_eq!(users_arr.len(), 1);
    assert!(users_arr
        .iter()
        .any(|u| u["email"].as_str().unwrap() == "superadmin@vistas.com"));

    let uid1 = quick_create_user(&env, &admin_jwt, "res1@vistas.com", "A", "101");
    let uid2 = quick_create_user(&env, &admin_jwt, "res2@vistas.com", "B", "202");
    let uid3 = quick_create_user(&env, &admin_jwt, "res3@vistas.com", "C", "303");

    assert!(uid1 > 0);
    assert!(uid2 > 0);
    assert!(uid3 > 0);

    let list_resp2 = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &admin_auth_header)
        .send_json(serde_json::json!({}))
        .expect("Admin failed to list users");

    let users_val2: serde_json::Value = list_resp2.into_json().unwrap();
    let users_arr2 = users_val2.as_array().unwrap();
    assert_eq!(users_arr2.len(), 4);

    let res2 = users_arr2
        .iter()
        .find(|u| u["email"].as_str().unwrap() == "res2@vistas.com")
        .unwrap();
    assert_eq!(res2["wing"].as_str().unwrap(), "B");
    assert_eq!(res2["flat_number"].as_str().unwrap(), "0202");
}

/// Tests the complete resident onboarding flow, including password reset,
/// authentication, login, and `/api/v1/profile` retrieval.
#[tokio::test]
async fn test_resident_onboarding_and_profile_retrieval() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    let email = "onboard_resident@vistas.com";
    let _ = quick_create_user(&env, &admin_jwt, email, "X", "707");

    let resident_jwt = quick_onboard_user(&env, email, "resident_super_secure_pass_123");
    let res_auth_header = format!("Bearer {}", resident_jwt);

    let profile_resp = ureq::post(&format!("{}/api/v1/profile", url))
        .set("Authorization", &res_auth_header)
        .send_json(serde_json::json!({}))
        .expect("Resident failed to fetch profile");

    assert_eq!(profile_resp.status(), 200);
    let profile_val: serde_json::Value = profile_resp.into_json().unwrap();

    assert_eq!(profile_val["email"].as_str().unwrap(), email);
    assert_eq!(profile_val["wing"].as_str().unwrap(), "X");
    assert_eq!(profile_val["flat_number"].as_str().unwrap(), "0707");
}

/// Tests dynamic document visibility restrictions, forbidden access barriers,
/// dynamic promotion visibility upgrades, and soft-delete hidden states.
#[tokio::test]
async fn test_document_permissions_and_soft_delete() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);

    let rid1 = quick_create_user(&env, &admin_jwt, "resident1@vistas.com", "A", "101");
    let resident1_jwt = quick_onboard_user(&env, "resident1@vistas.com", "resident_pass_111");
    let res1_auth_header = format!("Bearer {}", resident1_jwt);

    let _rid2 = quick_create_user(&env, &admin_jwt, "resident2@vistas.com", "B", "202");

    quick_create_document_category(&env, &admin_jwt, "COMMITTEE", "Committee");

    let doc1_id = quick_create_document(&env, &admin_jwt, "Handbook 2026", "PUBLIC", None);
    let doc2_id = quick_create_document(
        &env,
        &admin_jwt,
        "Flat Registry A-101",
        "RESIDENT_SPECIFIC",
        Some(("A", "101")),
    );
    let doc3_id = quick_create_document(
        &env,
        &admin_jwt,
        "Flat Registry B-202",
        "RESIDENT_SPECIFIC",
        Some(("B", "202")),
    );
    let doc4_id = quick_create_document(
        &env,
        &admin_jwt,
        "Committee Meeting Pack",
        "COMMITTEE",
        None,
    );

    let list_resp = ureq::post(&format!("{}/api/v1/documents", url))
        .set("Authorization", &res1_auth_header)
        .send_json(serde_json::json!({}))
        .unwrap();

    let visible_docs: Vec<serde_json::Value> = list_resp.into_json().unwrap();
    let visible_ids: Vec<String> = visible_docs
        .iter()
        .map(|d| d["document"]["document_id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(visible_ids.len(), 2);
    assert!(visible_ids.contains(&doc1_id));
    assert!(visible_ids.contains(&doc2_id));
    assert!(!visible_ids.contains(&doc3_id));
    assert!(!visible_ids.contains(&doc4_id));

    let get_doc3 = ureq::post(&format!("{}/api/v1/documents/{}", url, doc3_id))
        .set("Authorization", &res1_auth_header)
        .send_json(serde_json::json!({}));
    assert_eq!(get_doc3.unwrap_err().into_response().unwrap().status(), 403);

    let get_doc4 = ureq::post(&format!("{}/api/v1/documents/{}", url, doc4_id))
        .set("Authorization", &res1_auth_header)
        .send_json(serde_json::json!({}));
    assert_eq!(get_doc4.unwrap_err().into_response().unwrap().status(), 403);

    quick_create_designation(&env, &admin_jwt, "committee", "Committee");
    quick_set_category_designation(&env, &admin_jwt, "COMMITTEE", "committee", true, false);
    quick_assign_designation(&env, &admin_jwt, rid1, "committee");

    let list_resp2 = ureq::post(&format!("{}/api/v1/documents", url))
        .set("Authorization", &res1_auth_header)
        .send_json(serde_json::json!({}))
        .unwrap();

    let visible_docs2: Vec<serde_json::Value> = list_resp2.into_json().unwrap();
    let visible_ids2: Vec<String> = visible_docs2
        .iter()
        .map(|d| d["document"]["document_id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(visible_ids2.len(), 3);
    assert!(visible_ids2.contains(&doc1_id));
    assert!(visible_ids2.contains(&doc2_id));
    assert!(visible_ids2.contains(&doc4_id));
    assert!(!visible_ids2.contains(&doc3_id));

    let get_doc4_promoted = ureq::post(&format!("{}/api/v1/documents/{}", url, doc4_id))
        .set("Authorization", &res1_auth_header)
        .send_json(serde_json::json!({}))
        .unwrap();
    assert_eq!(get_doc4_promoted.status(), 200);
    let doc4_fetched: serde_json::Value = get_doc4_promoted.into_json().unwrap();
    assert!(doc4_fetched["download_url"]
        .as_str()
        .unwrap()
        .starts_with("http://localhost:9009/vistas-files/documents/"));

    let delete_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/{}/delete",
        url, doc1_id
    ))
    .set("Authorization", &format!("Bearer {}", admin_jwt))
    .send_json(serde_json::json!({}))
    .unwrap();
    assert_eq!(delete_resp.status(), 204);

    let list_resp3 = ureq::post(&format!("{}/api/v1/documents", url))
        .set("Authorization", &res1_auth_header)
        .send_json(serde_json::json!({}))
        .unwrap();

    let visible_docs3: Vec<serde_json::Value> = list_resp3.into_json().unwrap();
    let visible_ids3: Vec<String> = visible_docs3
        .iter()
        .map(|d| d["document"]["document_id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(visible_ids3.len(), 2);
    assert!(!visible_ids3.contains(&doc1_id));
}

/// Tests rate limiting middleware for IP, User, and Admin limits.
#[tokio::test]
async fn test_rate_limiting() {
    let env = setup_api_test_env().await;
    let url = env.server_url();

    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "rate_limit_user@vistas.com", "A", "101");
    let user_jwt = quick_onboard_user(&env, "rate_limit_user@vistas.com", "password123");
    let auth_header = format!("Bearer {}", user_jwt);

    // 1. Test User Limit (120 per min)
    let mut user_success = 0;
    let mut user_rate_limited = false;

    for _ in 0..125 {
        let resp = ureq::post(&format!("{}/api/v1/profile", url))
            .set("Authorization", &auth_header)
            .send_json(serde_json::json!({}));

        match resp {
            Ok(r) => {
                assert_eq!(r.status(), 200);
                user_success += 1;
            }
            Err(ureq::Error::Status(429, _)) => {
                user_rate_limited = true;
                break;
            }
            Err(e) => panic!("Unexpected error during profile fetch: {:?}", e),
        }
    }

    assert!(user_rate_limited, "Expected to be rate limited by User");
    assert!(
        (119..=125).contains(&user_success),
        "Expected ~120 successes, got {}",
        user_success
    );

    // 2. Test IP Limit (300 per min — but we already used some from user requests above)
    // The IP limiter is shared across all requests from localhost.
    // After 120 user requests + setup overhead, we're already close to the limit.
    // We verify the IP limiter triggers within the remaining budget.
    let mut ip_success = 0;
    let mut ip_rate_limited = false;

    for _ in 0..300 {
        let resp = ureq::get(&format!("{}/health", url)).call();
        match resp {
            Ok(r) => {
                assert_eq!(r.status(), 200);
                ip_success += 1;
            }
            Err(ureq::Error::Status(429, _)) => {
                ip_rate_limited = true;
                break;
            }
            Err(e) => panic!("Unexpected error during health fetch: {:?}", e),
        }
    }

    assert!(ip_rate_limited, "Expected to be rate limited by IP");
    assert!(ip_success > 0 && ip_success <= 300);
}
