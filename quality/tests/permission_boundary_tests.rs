//! Dedicated authorization-boundary coverage: multiple users holding different
//! designations/permissions, asserting both what they CAN and what they explicitly
//! CANNOT do via the API. This is the real security surface of the RBAC refactor —
//! every other test file assumes authorization already works; this one proves it.

use quality::{
    quick_assign_designation, quick_create_designation, quick_create_document,
    quick_create_document_category, quick_create_user, quick_onboard_user,
    quick_revoke_designation, quick_set_category_designation, quick_setup_admin,
    quick_toggle_designation_permission, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn assert_forbidden(resp: Result<ureq::Response, ureq::Error>, context: &str) {
    assert!(resp.is_err(), "expected failure for {}", context);
    assert_eq!(
        resp.unwrap_err().into_response().unwrap().status(),
        403,
        "expected 403 for {}",
        context
    );
}

fn assert_error(resp: Result<ureq::Response, ureq::Error>, context: &str) {
    assert!(resp.is_err(), "expected failure for {}", context);
}

/// Seeds a brand-new designation holding exactly one permission — used to prove
/// authorization is genuinely per-permission, not all-or-nothing.
async fn seed_scoped_designation(env: &TestEnv, code: &str, permission_code: &str) {
    sqlx::query("INSERT INTO designations (code, label) VALUES ($1, $1) ON CONFLICT DO NOTHING")
        .bind(code)
        .execute(&env.db_pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO designation_permissions (designation_id, permission_id) \
         SELECT d.designation_id, p.permission_id FROM designations d, permissions p \
         WHERE d.code = $1 AND p.code = $2 \
         ON CONFLICT DO NOTHING",
    )
    .bind(code)
    .bind(permission_code)
    .execute(&env.db_pool)
    .await
    .unwrap();
}

/// A resident with no designations gets 403 on every admin route group, including
/// the new /roles/* group.
#[tokio::test]
async fn test_resident_gets_403_on_all_admin_routes() {
    let env = setup_api_test_env(3140).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let rid = quick_create_user(&env, &admin_jwt, "plain_res@test.com", "A", "101");
    let res_jwt = quick_onboard_user(&env, "plain_res@test.com", "password123");
    let res_auth = auth(&res_jwt);

    let checks: Vec<(&str, serde_json::Value)> = vec![
        ("/api/v1/admin/users/list", serde_json::json!({})),
        (
            "/api/v1/admin/users/create",
            serde_json::json!({"email": "x@test.com", "wing": "A", "flat_number": "999"}),
        ),
        (
            "/api/v1/admin/process/review",
            serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "is_approved": true}),
        ),
        ("/api/v1/admin/roles/designations", serde_json::json!({})),
        (
            "/api/v1/admin/roles/designations/assign",
            serde_json::json!({"user_id": rid, "designation_code": "committee"}),
        ),
        (
            "/api/v1/admin/roles/document-category-designations",
            serde_json::json!({}),
        ),
    ];

    for (path, body) in checks {
        let resp = ureq::post(&format!("{}{}", url, path))
            .set("Authorization", &res_auth)
            .send_json(body);
        assert_forbidden(resp, path);
    }

    // /documents/list has no router-level gate (access is per-category inside the handler),
    // so a resident with no manageable categories gets 200 with an empty list, not 403.
    let docs_resp = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap();
    let docs: Vec<serde_json::Value> = docs_resp.into_json().unwrap();
    assert!(docs.is_empty());
}

/// `committee` holds zero rows in `designation_permissions` today (only the document
/// category mapping) — proves that having *a* designation doesn't grant admin-ish
/// treatment anywhere; it still gets 403 on every admin route.
#[tokio::test]
async fn test_committee_gets_403_on_all_admin_routes() {
    let env = setup_api_test_env(3141).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let rid = quick_create_user(&env, &admin_jwt, "committee_res@test.com", "A", "102");
    let res_jwt = quick_onboard_user(&env, "committee_res@test.com", "password123");
    quick_create_designation(&env, &admin_jwt, "committee", "Committee");
    quick_assign_designation(&env, &admin_jwt, rid, "committee");
    let committee_auth = auth(&res_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
        .set("Authorization", &committee_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(resp, "/api/v1/admin/roles/designations");

    let resp2 = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &committee_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(resp2, "/api/v1/admin/users/list");
}

/// A designation holding exactly `users.roles.manage` (and nothing else) can call the
/// /roles/* group, but still gets 403 on routes gated by other permissions. Proves
/// gating is genuinely per-permission, not a single coarse admin/non-admin split.
#[tokio::test]
async fn test_scoped_permission_grants_only_matching_routes() {
    let env = setup_api_test_env(3142).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    seed_scoped_designation(&env, "roles_only_tester", "users.roles.manage").await;

    let rid = quick_create_user(&env, &admin_jwt, "scoped_res@test.com", "A", "103");
    let res_jwt = quick_onboard_user(&env, "scoped_res@test.com", "password123");
    quick_assign_designation(&env, &admin_jwt, rid, "roles_only_tester");
    let scoped_auth = auth(&res_jwt);

    let ok_resp = ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
        .set("Authorization", &scoped_auth)
        .send_json(serde_json::json!({}));
    assert!(
        ok_resp.is_ok(),
        "expected success for /roles/designations with users.roles.manage"
    );

    let denied_users = ureq::post(&format!("{}/api/v1/admin/users/create", url))
        .set("Authorization", &scoped_auth)
        .send_json(serde_json::json!({"email": "y@test.com", "wing": "A", "flat_number": "777"}));
    assert_forbidden(denied_users, "/api/v1/admin/users/create (users.manage)");

    let denied_process = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &scoped_auth)
        .send_json(serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "is_approved": true}));
    assert_forbidden(
        denied_process,
        "/api/v1/admin/process/review (process.reviewer)",
    );

    // No router-level gate on /documents/list — a caller with no manageable categories
    // gets 200 with an empty list, not 403.
    let docs_resp = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &scoped_auth)
        .send_json(serde_json::json!({}))
        .unwrap();
    let docs: Vec<serde_json::Value> = docs_resp.into_json().unwrap();
    assert!(docs.is_empty());
}

/// super_admin holds all 13 permissions — regression check that every route group is
/// still reachable after replacing the blanket admin_guard with per-permission gates.
#[tokio::test]
async fn test_super_admin_can_access_all_admin_route_groups() {
    let env = setup_api_test_env(3143).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    for path in [
        "/api/v1/admin/users/list",
        "/api/v1/admin/documents/list",
        "/api/v1/admin/roles/designations",
        "/api/v1/admin/roles/document-category-designations",
    ] {
        let resp = ureq::post(&format!("{}{}", url, path))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}));
        assert!(resp.is_ok(), "expected success for {}", path);
    }
}

/// Assigning/revoking a designation via /roles/designations/* has a real consequence:
/// it changes what documents the user can see, resolved through
/// `document_category_designations` at request time (not cached).
#[tokio::test]
async fn test_assign_revoke_designation_changes_document_visibility() {
    let env = setup_api_test_env(3144).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "COMMITTEE", "Committee");
    let doc_id = quick_create_document(&env, &admin_jwt, "Committee Minutes", "COMMITTEE", None);

    quick_create_designation(&env, &admin_jwt, "committee", "Committee");
    quick_set_category_designation(&env, &admin_jwt, "COMMITTEE", "committee", true, false);

    let rid = quick_create_user(&env, &admin_jwt, "vis_res@test.com", "A", "104");
    let res_jwt = quick_onboard_user(&env, "vis_res@test.com", "password123");
    let res_auth = auth(&res_jwt);

    let before = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(before, "document access before committee assignment");

    quick_assign_designation(&env, &admin_jwt, rid, "committee");

    let after = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(after.is_ok(), "expected access after committee assignment");

    quick_revoke_designation(&env, &admin_jwt, rid, "committee");

    let after_revoke = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(after_revoke, "document access after committee revocation");
}

/// `super_admin` is an `is_system` designation, seed-only — assigning or revoking it
/// through the generic /roles/designations/* endpoints must be rejected, replacing the
/// old hardcoded "committee is the only valid target" check with a schema-driven one.
#[tokio::test]
async fn test_cannot_assign_or_revoke_system_designation() {
    let env = setup_api_test_env(3145).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let rid = quick_create_user(&env, &admin_jwt, "sysdes_res@test.com", "A", "105");
    let _res_jwt = quick_onboard_user(&env, "sysdes_res@test.com", "password123");

    let assign_resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"user_id": rid, "designation_code": "super_admin"}));
    assert_forbidden(
        assign_resp,
        "assigning super_admin via /roles/designations/assign",
    );

    let revoke_resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/revoke", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"user_id": rid, "designation_code": "super_admin"}));
    assert_forbidden(
        revoke_resp,
        "revoking super_admin via /roles/designations/revoke",
    );
}

/// A caller can never modify their own designations through /roles/designations/* —
/// preserves the self-demotion protection the old `update_role` handler had, now
/// generalized to any designation rather than just "committee".
#[tokio::test]
async fn test_cannot_modify_own_designations() {
    let env = setup_api_test_env(3146).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let profile_val: serde_json::Value = ureq::post(&format!("{}/api/v1/profile", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let own_id = profile_val["user_id"].as_i64().unwrap() as i32;

    let resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"user_id": own_id, "designation_code": "committee"}));
    assert_forbidden(resp, "assigning a designation to yourself");
}

/// Toggling a `document_category_designations` cell via
/// /roles/document-category-designations/toggle is reflected in real document
/// visibility for a user holding the matching designation.
#[tokio::test]
async fn test_category_matrix_toggle_reflects_in_visibility() {
    let env = setup_api_test_env(3147).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "FINANCE", "Finance");
    let doc_id = quick_create_document(&env, &admin_jwt, "Finance Report", "FINANCE", None);

    let rid = quick_create_user(&env, &admin_jwt, "matrix_res@test.com", "A", "106");
    let res_jwt = quick_onboard_user(&env, "matrix_res@test.com", "password123");
    let res_auth = auth(&res_jwt);

    quick_create_designation(&env, &admin_jwt, "matrix_staff", "Matrix Staff");
    quick_assign_designation(&env, &admin_jwt, rid, "matrix_staff");

    // No category is mapped to any designation until explicitly toggled — forbidden.
    let before = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(before, "finance doc access before matrix toggle");

    quick_set_category_designation(&env, &admin_jwt, "FINANCE", "matrix_staff", true, false);

    let after = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(
        after.is_ok(),
        "expected access after enabling FINANCE for staff"
    );

    quick_set_category_designation(&env, &admin_jwt, "FINANCE", "matrix_staff", false, false);

    let after_off = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(
        after_off,
        "finance doc access after disabling the mapping again",
    );
}

/// End-to-end: create a brand-new designation, assign it to a user, create a brand-new
/// document category, upload a document into it, then grant the designation read access —
/// the user gains visibility only after the grant, not before.
#[tokio::test]
async fn test_full_dynamic_designation_and_category_flow() {
    let env = setup_api_test_env(3148).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    // 1. Super admin creates a new designation via the API.
    quick_create_designation(&env, &admin_jwt, "concierge", "Concierge");

    // 2. Super admin adds a new user to this designation.
    let rid = quick_create_user(&env, &admin_jwt, "concierge_res@test.com", "A", "107");
    let res_jwt = quick_onboard_user(&env, "concierge_res@test.com", "password123");
    let res_auth = auth(&res_jwt);
    quick_assign_designation(&env, &admin_jwt, rid, "concierge");

    let listed = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .unwrap();
    let users: Vec<serde_json::Value> = listed.into_json().unwrap();
    let user_row = users
        .iter()
        .find(|u| u["user_id"].as_i64() == Some(rid as i64))
        .unwrap();
    assert!(user_row["designations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d.as_str() == Some("concierge")));

    // 3. Create a brand-new document category and upload a document into it.
    quick_create_document_category(&env, &admin_jwt, "VISITOR_LOGS", "Visitor Logs");
    let doc_id = quick_create_document(&env, &admin_jwt, "Visitor Log — Jan", "VISITOR_LOGS", None);

    // Not yet visible: no designation has been granted read access to this category.
    let before = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(
        before,
        "visitor logs access before granting concierge read access",
    );

    // 4. Grant the "concierge" designation read access to the new category.
    quick_set_category_designation(&env, &admin_jwt, "VISITOR_LOGS", "concierge", true, false);

    // 5. Now the user can see it.
    let after = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(
        after.is_ok(),
        "expected visitor logs access after granting concierge read access"
    );
}

/// Creating a designation with a code that already exists is rejected, not silently
/// merged or duplicated.
#[tokio::test]
async fn test_duplicate_designation_code_rejected() {
    let env = setup_api_test_env(3149).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_designation(&env, &admin_jwt, "auditor", "Auditor");

    let dup = ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"code": "auditor", "label": "Auditor Again"}));
    assert_error(dup, "duplicate designation code");

    let designations: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(
        designations
            .iter()
            .filter(|d| d["code"] == "auditor")
            .count(),
        1
    );
}

/// Same for document categories: duplicate code is rejected.
#[tokio::test]
async fn test_duplicate_category_code_rejected() {
    let env = setup_api_test_env(3150).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "NOTICES", "Notices");

    let dup = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"code": "NOTICES", "label": "Notices Again"}));
    assert_error(dup, "duplicate category code");
}

/// Assigning a designation code that doesn't exist is rejected with a clear error,
/// not silently accepted or turned into an FK-violation 500.
#[tokio::test]
async fn test_assign_unknown_designation_fails() {
    let env = setup_api_test_env(3151).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let rid = quick_create_user(&env, &admin_jwt, "unknown_desig@test.com", "A", "108");

    let resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"user_id": rid, "designation_code": "does_not_exist"}));
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("expected 400, got: {:?}", other),
    }
}

/// Revoking a designation from a user who never had it is a graceful no-op, not an error.
#[tokio::test]
async fn test_revoke_designation_user_never_had_is_noop() {
    let env = setup_api_test_env(3152).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_designation(&env, &admin_jwt, "auditor", "Auditor");
    let rid = quick_create_user(&env, &admin_jwt, "never_had@test.com", "A", "109");

    let resp = ureq::post(&format!("{}/api/v1/admin/roles/designations/revoke", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"user_id": rid, "designation_code": "auditor"}));
    assert!(
        resp.is_ok(),
        "revoking a designation never held should be a no-op success"
    );
}

/// `can_manage` and `can_read` are independent: a designation granted manage-only access
/// can create/manage documents in a category via the admin API, but that same document
/// never shows up through the resident-facing read endpoints, which only check `can_read`.
#[tokio::test]
async fn test_manage_without_read_can_write_but_not_view() {
    let env = setup_api_test_env(3153).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "ARCHIVES", "Archives");
    quick_create_designation(&env, &admin_jwt, "archivist", "Archivist");
    quick_set_category_designation(&env, &admin_jwt, "ARCHIVES", "archivist", false, true);

    let rid = quick_create_user(&env, &admin_jwt, "archivist_res@test.com", "A", "110");
    let res_jwt = quick_onboard_user(&env, "archivist_res@test.com", "password123");
    let res_auth = auth(&res_jwt);
    quick_assign_designation(&env, &admin_jwt, rid, "archivist");

    // Can create a document in the category via the admin/manage path.
    let doc_id = quick_create_document(&env, &res_jwt, "Old Minutes", "ARCHIVES", None);

    // But cannot see it via the resident-facing read endpoint (can_read is false).
    let resident_view = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert_forbidden(
        resident_view,
        "resident read of manage-only category document",
    );

    // The admin management view (manage grant, not can_read) still shows it.
    let admin_view = ureq::post(&format!("{}/api/v1/admin/documents/{}/get", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(
        admin_view.is_ok(),
        "manage-only designation should still see it via the admin view"
    );
}

/// A category still containing documents cannot be deleted.
#[tokio::test]
async fn test_cannot_delete_category_with_existing_documents() {
    let env = setup_api_test_env(3154).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "LEASES", "Leases");
    quick_create_document(&env, &admin_jwt, "Lease Agreement", "LEASES", None);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/delete",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"code": "LEASES"}));
    assert_error(resp, "deleting a category with existing documents");
}

/// An empty (unused) category can be deleted.
#[tokio::test]
async fn test_delete_empty_category_succeeds() {
    let env = setup_api_test_env(3155).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "TEMP_CAT", "Temp");

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/delete",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"code": "TEMP_CAT"}));
    assert!(resp.is_ok(), "deleting an empty category should succeed");

    let categories: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/roles/document-categories", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert!(!categories.iter().any(|c| c["code"] == "TEMP_CAT"));
}

/// PUBLIC/RESIDENT_SPECIFIC are protected system categories and can never be deleted.
#[tokio::test]
async fn test_cannot_delete_system_category() {
    let env = setup_api_test_env(3156).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    for code in ["PUBLIC", "RESIDENT_SPECIFIC"] {
        let resp = ureq::post(&format!(
            "{}/api/v1/admin/roles/document-categories/delete",
            url
        ))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({"code": code}));
        assert_error(resp, code);
    }
}

/// super_admin's permission set is fixed — attempting to toggle a permission on it
/// through the generic designation-permission endpoint is rejected.
#[tokio::test]
async fn test_cannot_toggle_permissions_for_system_designation() {
    let env = setup_api_test_env(3157).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/roles/designations/permissions/toggle",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({"designation_code": "super_admin", "permission_code": "process.view", "enabled": false}));
    assert_forbidden(resp, "toggling a permission on super_admin");
}

/// A holder of `designations.manage` can never toggle permissions on a designation they
/// themselves currently hold — otherwise they could grant that designation (and
/// therefore themselves) any other permission, a self-escalation path entirely separate
/// from the assign/revoke self-edit guard. They CAN still edit a different designation's
/// permissions, proving the block is scoped to "held by the caller", not a blanket denial.
#[tokio::test]
async fn test_cannot_edit_permissions_of_own_designation() {
    let env = setup_api_test_env(3176).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    seed_scoped_designation(&env, "self_edit_tester", "designations.manage").await;
    quick_create_designation(&env, &admin_jwt, "other_designation", "Other Designation");

    let rid = quick_create_user(&env, &admin_jwt, "self_edit_res@test.com", "A", "112");
    let res_jwt = quick_onboard_user(&env, "self_edit_res@test.com", "password123");
    quick_assign_designation(&env, &admin_jwt, rid, "self_edit_tester");
    let res_auth = auth(&res_jwt);

    // Blocked: this designation is held by the caller.
    let self_edit = ureq::post(&format!(
        "{}/api/v1/admin/roles/designations/permissions/toggle",
        url
    ))
    .set("Authorization", &res_auth)
    .send_json(serde_json::json!({"designation_code": "self_edit_tester", "permission_code": "users.manage", "enabled": true}));
    assert_forbidden(
        self_edit,
        "toggling a permission on a designation held by the caller",
    );

    // Allowed: a different designation, not held by the caller.
    let other_edit = ureq::post(&format!(
        "{}/api/v1/admin/roles/designations/permissions/toggle",
        url
    ))
    .set("Authorization", &res_auth)
    .send_json(serde_json::json!({"designation_code": "other_designation", "permission_code": "users.view", "enabled": true}));
    assert!(
        other_edit.is_ok(),
        "editing a designation not held by the caller should still succeed"
    );
}

/// Designation codes are normalized (trimmed, lowercased, spaces -> underscores) so
/// visually-different inputs can't create near-duplicate codes.
#[tokio::test]
async fn test_designation_code_is_normalized() {
    let env = setup_api_test_env(3158).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
            .set("Authorization", &admin_auth)
            .send_json(
                serde_json::json!({"code": "  Facility Manager  ", "label": "Facility Manager"}),
            )
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(resp["code"], "facility_manager");
}

/// Granting a non-super_admin designation the blanket `documents.all.manage` permission
/// bypasses per-category checks entirely, same as super_admin.
#[tokio::test]
async fn test_documents_all_manage_bypasses_per_category_checks() {
    let env = setup_api_test_env(3159).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_designation(&env, &admin_jwt, "super_staff", "Super Staff");
    quick_toggle_designation_permission(
        &env,
        &admin_jwt,
        "super_staff",
        "documents.all.manage",
        true,
    );

    let rid = quick_create_user(&env, &admin_jwt, "super_staff_res@test.com", "A", "111");
    let res_jwt = quick_onboard_user(&env, "super_staff_res@test.com", "password123");
    quick_assign_designation(&env, &admin_jwt, rid, "super_staff");

    // A category this designation was never explicitly granted access to.
    quick_create_document_category(&env, &admin_jwt, "UNMAPPED", "Unmapped");
    let doc_id = quick_create_document(&env, &admin_jwt, "Unmapped Doc", "UNMAPPED", None);

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/{}/get", url, doc_id))
        .set("Authorization", &format!("Bearer {}", res_jwt))
        .send_json(serde_json::json!({}));
    assert!(
        resp.is_ok(),
        "documents.all.manage should bypass per-category checks even for non-super_admin"
    );
}
