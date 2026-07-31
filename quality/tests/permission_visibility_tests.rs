//! Resource-visibility matrix: for each permission, what documents/processes/residents
//! does a designation holding *only* that permission actually see? Complements
//! `route_permission_matrix_tests.rs` (which checks route-level 403 vs allowed) and
//! `permission_boundary_tests.rs` (business logic) by asserting on response *content*,
//! not just status codes — a permission can pass a route gate and still leak (or hide)
//! resources it shouldn't.

use quality::{
    quick_assign_designation, quick_create_document, quick_create_user, quick_onboard_user,
    quick_set_category_designation, quick_setup_admin, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn seeded_resident(
    env: &TestEnv,
    admin_jwt: &str,
    tag: &str,
    flat_number: &str,
    permission_codes: &[&str],
) -> String {
    let url = env.server_url();
    let designation_code = format!("vis_{}", tag);
    let email = format!("{}@visibility.test", tag);

    ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({"code": designation_code, "label": designation_code}))
        .expect("create designation");

    for perm in permission_codes {
        ureq::post(&format!(
            "{}/api/v1/admin/roles/designations/permissions/toggle",
            url
        ))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({
            "designation_code": designation_code,
            "permission_code": perm,
            "enabled": true
        }))
        .expect("toggle permission");
    }

    let rid = quick_create_user(env, admin_jwt, &email, "V", flat_number);
    let res_jwt = quick_onboard_user(env, &email, "password123");
    quick_assign_designation(env, admin_jwt, rid, &designation_code);
    res_jwt
}

/// `users.manage`-only holder sees no documents (empty list, not 403 — no router-level
/// gate on /admin/documents/list) and no other resident's processes.
#[tokio::test]
async fn test_users_manage_grants_no_document_or_process_visibility() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document(&env, &admin_jwt, "Public Notice", "PUBLIC", None);

    let jwt = seeded_resident(&env, &admin_jwt, "usermgr", "1", &["users.manage"]);
    let a = auth(&jwt);

    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        docs.is_empty(),
        "users.manage alone must not surface any documents"
    );

    let err = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &a)
        .send_json(
            serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "is_approved": true}),
        );
    assert!(
        err.is_err(),
        "users.manage alone must not grant process review access"
    );
}

/// `documents.all.manage`-only holder sees every document but no resident list and no
/// process review access.
#[tokio::test]
async fn test_documents_all_manage_grants_no_user_or_process_visibility() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/create",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({"code": "VIS_CAT_ALL", "label": "Vis Cat All"}))
    .unwrap();

    quick_create_document(&env, &admin_jwt, "Doc A", "PUBLIC", None);
    quick_create_document(&env, &admin_jwt, "Doc B", "VIS_CAT_ALL", None);

    let jwt = seeded_resident(&env, &admin_jwt, "docall", "2", &["documents.all.manage"]);
    let a = auth(&jwt);

    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        docs.len() >= 2,
        "documents.all.manage must see documents across every category"
    );

    assert!(
        ureq::post(&format!("{}/api/v1/admin/users/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({}))
            .is_err(),
        "documents.all.manage alone must not grant resident-list access"
    );
    assert!(
        ureq::post(&format!("{}/api/v1/admin/process/review", url))
            .set("Authorization", &a)
            .send_json(
                serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "is_approved": true}),
            )
            .is_err(),
        "documents.all.manage alone must not grant process review access"
    );
}

/// A designation scoped to a single category (via the category-designation matrix, not
/// `documents.all.manage`) sees only documents in that category, not others.
#[tokio::test]
async fn test_category_scoped_designation_sees_only_its_category() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/create",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({"code": "VIS_CAT_A", "label": "Vis Cat A"}))
    .unwrap();
    ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/create",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({"code": "VIS_CAT_B", "label": "Vis Cat B"}))
    .unwrap();

    quick_create_document(&env, &admin_jwt, "Cat A Doc", "VIS_CAT_A", None);
    quick_create_document(&env, &admin_jwt, "Cat B Doc", "VIS_CAT_B", None);

    ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"code": "vis_cat_a_mgr", "label": "Vis Cat A Mgr"}))
        .unwrap();
    quick_set_category_designation(&env, &admin_jwt, "VIS_CAT_A", "vis_cat_a_mgr", true, true);

    let rid = quick_create_user(&env, &admin_jwt, "catmgr@visibility.test", "V", "3");
    let jwt = quick_onboard_user(&env, "catmgr@visibility.test", "password123");
    quick_assign_designation(&env, &admin_jwt, rid, "vis_cat_a_mgr");
    let a = auth(&jwt);

    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();

    assert!(
        docs.iter()
            .all(|d| d["document"]["category"] == "VIS_CAT_A"),
        "category-scoped designation must only see documents in its own category, got: {:?}",
        docs
    );
    assert!(
        !docs.is_empty(),
        "category-scoped designation must see documents in its own category"
    );
}

/// `process.view`-only holder can list every resident's processes (read-only oversight)
/// but cannot approve/reject a step (write-gated to `process.reviewer`).
#[tokio::test]
async fn test_process_view_lists_all_processes_but_cannot_review() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "owner_pv@test.com", "V", "4");
    let owner_jwt = quick_onboard_user(&env, "owner_pv@test.com", "password123");
    let init: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap()
        .into_json()
        .unwrap();
    let process_id = init["process_id"].as_str().unwrap();

    let jwt = seeded_resident(&env, &admin_jwt, "procview", "5", &["process.view"]);
    let a = auth(&jwt);

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let items = list["processes"].as_array().unwrap();
    assert!(
        items.iter().any(|p| p["process_id"] == process_id),
        "process.view must see other residents' processes"
    );

    let err = ureq::post(&format!("{}/api/v1/admin/process/review", url))
        .set("Authorization", &a)
        .send_json(
            serde_json::json!({"process_id": process_id, "template_step_id": 1, "is_approved": true}),
        );
    assert!(
        err.is_err(),
        "process.view alone must not be able to approve/reject a step"
    );
}

/// `process.reviewer`-only holder gets an empty list from the generic document admin
/// endpoint (no document-category grant), even though it can review every process.
#[tokio::test]
async fn test_process_reviewer_has_no_generic_document_list_access() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document(&env, &admin_jwt, "Unrelated Public Doc", "PUBLIC", None);

    let jwt = seeded_resident(&env, &admin_jwt, "reviewervis", "6", &["process.reviewer"]);
    let a = auth(&jwt);

    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(
        docs.is_empty(),
        "process.reviewer alone must not surface documents via the generic admin list"
    );
}

/// Super admin sees everything: all documents across categories, all processes, and the
/// full resident list — sanity check that the matrix above isn't hiding a global bypass.
#[tokio::test]
async fn test_super_admin_sees_all_documents_processes_and_residents() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let a = auth(&admin_jwt);

    quick_create_document(&env, &admin_jwt, "Super Visible Doc", "PUBLIC", None);
    quick_create_user(&env, &admin_jwt, "owner_sa@test.com", "V", "7");
    let owner_jwt = quick_onboard_user(&env, "owner_sa@test.com", "password123");
    let init: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &auth(&owner_jwt))
        .send_json(serde_json::json!({"template_id": 1}))
        .unwrap()
        .into_json()
        .unwrap();
    let process_id = init["process_id"].as_str().unwrap();

    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(docs
        .iter()
        .any(|d| d["document"]["title"] == "Super Visible Doc"));

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(list["processes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["process_id"] == process_id));

    let users: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/users/list", url))
        .set("Authorization", &a)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(users.iter().any(|u| u["email"] == "owner_sa@test.com"));
}
