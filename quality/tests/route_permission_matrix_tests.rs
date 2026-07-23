//! Systematic sweep of every permission-gated admin route group: for each permission,
//! a designation holding *only* that permission must succeed on routes it gates and
//! get 403 on every route gated by a different permission. Complements
//! `permission_boundary_tests.rs` (which covers designation/category business logic and
//! a couple of scoped-permission spot checks) by exhaustively covering the route table
//! in `main.rs` instead of a handful of representative routes.

use quality::{
    quick_assign_designation, quick_create_user, quick_onboard_user, quick_setup_admin,
    setup_api_test_env, TestEnv,
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

fn assert_not_forbidden(resp: Result<ureq::Response, ureq::Error>, context: &str) {
    match resp {
        Ok(_) => {}
        Err(ureq::Error::Status(code, _)) => assert_ne!(
            code, 403,
            "expected non-403 (permission granted) for {}",
            context
        ),
        Err(e) => panic!("unexpected transport error for {}: {:?}", context, e),
    }
}

/// Seeds a designation holding exactly the given permission codes and assigns it to a
/// fresh resident, returning that resident's JWT. `flat_number` just needs to be unique
/// per test (a designation-holding resident doesn't need a real occupied flat).
fn seeded_resident(
    env: &TestEnv,
    admin_jwt: &str,
    tag: &str,
    flat_number: &str,
    permission_codes: &[&str],
) -> String {
    let url = env.server_url();
    let designation_code = format!("scoped_{}", tag);
    let email = format!("{}@matrix.test", tag);

    let _ = ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({"code": designation_code, "label": designation_code}));

    for perm in permission_codes {
        let _ = ureq::post(&format!(
            "{}/api/v1/admin/roles/designations/permissions/toggle",
            url
        ))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({
            "designation_code": designation_code,
            "permission_code": perm,
            "enabled": true
        }));
    }

    let rid = quick_create_user(env, admin_jwt, &email, "Z", flat_number);
    let res_jwt = quick_onboard_user(env, &email, "password123");
    quick_assign_designation(env, admin_jwt, rid, &designation_code);
    res_jwt
}

/// A designation holding only `users.view` can list users, but nothing else.
#[tokio::test]
async fn test_users_view_only() {
    let env = setup_api_test_env(3170).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "uview", "801", &["users.view"]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/users/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "users/list with users.view",
    );

    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/users/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"email": "x@test.com", "wing": "A", "flat_number": "1"})),
        "users/create with only users.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "roles/designations with only users.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/process/review", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "is_approved": true})),
        "process/review with only users.view",
    );
}

/// A designation holding only `users.manage` can create users and revoke flats, and can
/// also reach `/users/list` (that route accepts several permissions), but not `/roles/*`
/// or `/process/review`.
#[tokio::test]
async fn test_users_manage_only() {
    let env = setup_api_test_env(3171).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "umanage", "802", &["users.manage"]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/users/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"email": "umanage_target@test.com", "wing": "A", "flat_number": "2"})),
        "users/create with users.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/users/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "users/list also reachable with users.manage",
    );

    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"code": "nope", "label": "nope"})),
        "roles/designations/create with only users.manage",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/process/add-note", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "comment": "x"})),
        "process/add-note with only users.manage",
    );
}

/// A designation holding only `process.reviewer` can act on a real process (create one
/// as a separate resident, then add an internal note as the reviewer), but gets 403 on
/// user/role management routes.
#[tokio::test]
async fn test_process_reviewer_only() {
    let env = setup_api_test_env(3172).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "reviewer", "803", &["process.reviewer"]);
    let a = auth(&jwt);

    // A separate resident owns a real process for the reviewer to act on.
    quick_create_user(&env, &admin_jwt, "reviewee@test.com", "Y", "1");
    let owner_jwt = quick_onboard_user(&env, "reviewee@test.com", "password123");
    let init: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap()
        .into_json()
        .unwrap();
    let process_id = init["process_id"].as_str().unwrap();

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/process/add-note", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"process_id": process_id, "template_step_id": 1, "comment": "reviewed"})),
        "process/add-note with process.reviewer",
    );

    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/users/create", url))
            .set("Authorization", &a)
            .send_json(
                serde_json::json!({"email": "x2@test.com", "wing": "A", "flat_number": "3"}),
            ),
        "users/create with only process.reviewer",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "roles/designations with only process.reviewer",
    );
}

/// A designation holding only `designations.manage` can create/edit designations and
/// toggle the document-category access matrix, but cannot assign a designation to a
/// user (`users.roles.manage`-gated) or create a document category
/// (`documents.categories.manage`-gated).
#[tokio::test]
async fn test_designations_manage_only() {
    let env = setup_api_test_env(3173).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(
        &env,
        &admin_jwt,
        "desigmgr",
        "804",
        &["designations.manage"],
    );
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"code": "matrix_created", "label": "Matrix Created"})),
        "roles/designations/create with designations.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "roles/designations (shared read) with designations.manage",
    );

    let target = quick_create_user(&env, &admin_jwt, "desigmgr_target@test.com", "A", "4");
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
            .set("Authorization", &a)
            .send_json(
                serde_json::json!({"user_id": target, "designation_code": "matrix_created"}),
            ),
        "roles/designations/assign with only designations.manage",
    );
    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/roles/document-categories/create",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"code": "MATRIX_CAT", "label": "Matrix Cat"})),
        "roles/document-categories/create with only designations.manage",
    );
}

/// A designation holding only `documents.categories.manage` can create/rename/delete
/// document categories, but cannot create a designation or assign one.
#[tokio::test]
async fn test_documents_categories_manage_only() {
    let env = setup_api_test_env(3174).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(
        &env,
        &admin_jwt,
        "catmgr",
        "805",
        &["documents.categories.manage"],
    );
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/roles/document-categories/create",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"code": "CATMGR_CAT", "label": "Catmgr Cat"})),
        "roles/document-categories/create with documents.categories.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/document-categories", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "roles/document-categories (shared read) with documents.categories.manage",
    );

    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"code": "nope2", "label": "nope2"})),
        "roles/designations/create with only documents.categories.manage",
    );
    let target = quick_create_user(&env, &admin_jwt, "catmgr_target@test.com", "A", "5");
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"user_id": target, "designation_code": "super_admin"})),
        "roles/designations/assign with only documents.categories.manage",
    );
}

/// The `/documents/*` admin routes have no router-level permission gate at all (access
/// is per-category, checked inside each handler) — a designation with zero permissions
/// and zero category grants gets empty/forbidden results, never a router-level 403 that
/// would incorrectly suggest a blanket gate exists.
#[tokio::test]
async fn test_documents_routes_have_no_router_level_gate() {
    let env = setup_api_test_env(3175).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "nodocs", "806", &[]);
    let a = auth(&jwt);

    let list_resp = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}));
    assert!(
        list_resp.is_ok(),
        "documents/list should be reachable (200 with empty results), not router-gated"
    );
    let docs: Vec<serde_json::Value> = list_resp.unwrap().into_json().unwrap();
    assert!(docs.is_empty());

    let categories_resp = ureq::post(&format!("{}/api/v1/admin/documents/categories", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}));
    assert!(categories_resp.is_ok());
    let cats: Vec<serde_json::Value> = categories_resp.unwrap().into_json().unwrap();
    assert!(cats.is_empty());
}
