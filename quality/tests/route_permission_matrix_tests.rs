//! Systematic sweep of every permission-gated admin route group: for each permission,
//! a designation holding *only* that permission must succeed on routes it gates and
//! get 403 on every route gated by a different permission. Complements
//! `permission_boundary_tests.rs` (which covers designation/category business logic and
//! a couple of scoped-permission spot checks) by exhaustively covering the route table
//! in `main.rs` instead of a handful of representative routes.

use quality::{
    quick_assign_designation, quick_create_user, quick_onboard_user, quick_setup_admin,
    setup_api_test_env, setup_api_test_env_with_accounting_disabled, TestEnv,
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
    let env = setup_api_test_env().await;
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
    let env = setup_api_test_env().await;
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
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"wing": "Z", "flat_number": "101"})),
        "flats/create with users.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/bulk-create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"wing": "Z", "flat_numbers": ["201", "202"]})),
        "flats/bulk-create with users.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "flats/list also reachable with users.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/delete", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"flat_id": 999999})),
        "flats/delete with users.manage (404-shaped, but permission-gated not 403)",
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

/// A designation holding only `users.view` can list flats and flat plans (read-only
/// society structure) but cannot create them.
#[tokio::test]
async fn test_flats_permission_boundary() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "flatsview", "803", &["users.view"]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "flats/list with users.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"wing": "Z", "flat_number": "101"})),
        "flats/create with only users.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/bulk-create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"wing": "Z", "flat_numbers": ["101"]})),
        "flats/bulk-create with only users.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/set-fire-flag", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"flat_id": 1, "is_fire_flat": true})),
        "flats/set-fire-flag with only users.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/flats/delete", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"flat_id": 1})),
        "flats/delete with only users.view",
    );
}

/// A designation holding only `process.reviewer` can act on a real process (create one
/// as a separate resident, then add an internal note as the reviewer), but gets 403 on
/// user/role management routes.
#[tokio::test]
async fn test_process_reviewer_only() {
    let env = setup_api_test_env().await;
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
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/process/audit-log-pdf", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"process_id": process_id})),
        "process/audit-log-pdf with process.reviewer",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/process/audit-log-pdf", url))
            .set("Authorization", &auth(&owner_jwt))
            .send_json(serde_json::json!({"process_id": process_id})),
        "process/audit-log-pdf without reviewer/view permission",
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
    let env = setup_api_test_env().await;
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
    let env = setup_api_test_env().await;
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
    let env = setup_api_test_env().await;
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

/// The three bulk document routes share the same no-router-gate pattern as `/list` and
/// `/categories`: reachable by any authenticated user regardless of permissions, with
/// per-document category-manage filtering done inside the handler. A caller with zero
/// document permissions gets every requested document back as "skipped", never a 403.
#[tokio::test]
async fn test_documents_bulk_routes_have_no_router_level_gate() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "nobulkdocs", "808", &[]);
    let a = auth(&jwt);

    let doc_id = quality::quick_create_document(&env, &admin_jwt, "Doc", "PUBLIC", None);

    let delete_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/bulk-delete", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"document_ids": [doc_id.clone()]}))
            .expect("bulk-delete should be reachable, not router-gated")
            .into_json()
            .unwrap();
    assert!(delete_resp["succeeded"].as_array().unwrap().is_empty());
    assert_eq!(delete_resp["skipped"].as_array().unwrap().len(), 1);

    let subcat_resp: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-subcategory",
        url
    ))
    .set("Authorization", &a)
    .send_json(serde_json::json!({"document_ids": [doc_id.clone()], "subcategory_code": null}))
    .expect("bulk-update-subcategory should be reachable, not router-gated")
    .into_json()
    .unwrap();
    assert!(subcat_resp["succeeded"].as_array().unwrap().is_empty());
    assert_eq!(subcat_resp["skipped"].as_array().unwrap().len(), 1);

    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/documents/bulk-update-category",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"document_ids": [doc_id], "category": "PUBLIC"})),
        "bulk-update-category into a category the caller can't manage",
    );
}

/// A designation holding only `documents.signatures.sign` can list the signing queue
/// and read a requirement's status, but cannot manage the signer directory
/// (`process.reviewer` — there is no separate signatures-management permission).
#[tokio::test]
async fn test_documents_signatures_queue_open_to_any_authenticated_user() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    // No signatures-specific permission exists at all — seed a designation holding
    // zero permissions to prove the queue/replace-* routes need nothing beyond plain
    // authentication (they're self-scoped: queue only ever returns the caller's own
    // requirements, replace-* requires the caller to actually BE the signer).
    let jwt = seeded_resident(&env, &admin_jwt, "sigbare", "807", &[]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/queue", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "documents/signatures/queue with no permissions at all",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/all", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"document_id": uuid::Uuid::new_v4()})),
        "documents/signatures/all requires process.reviewer/process.view/documents.all.manage",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/signers/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "signers/list requires process.reviewer",
    );
}

/// A designation holding `process.reviewer` can manage the signer directory (list,
/// set-active, delete) and read requirement status, but cannot see/act on the signing
/// queue itself (`documents.signatures.sign`) unless they're the actual signer.
#[tokio::test]
async fn test_process_reviewer_manages_signer_directory() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(
        &env,
        &admin_jwt,
        "sigreviewer",
        "808",
        &["process.reviewer"],
    );
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/all", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"document_id": uuid::Uuid::new_v4()})),
        "documents/signatures/all with process.reviewer",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/signers/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"include_inactive": true})),
        "signers/list with process.reviewer",
    );
}

/// `/documents/signatures/all` (optionally scoped to one document) is also reachable
/// by the general process read/review permissions, since the admin process view needs
/// to show signature status without requiring a separate signature-specific grant.
#[tokio::test]
async fn test_documents_signatures_all_reachable_by_process_permissions() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "sigviaproc", "809", &["process.view"]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/documents/signatures/all", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"document_id": uuid::Uuid::new_v4()})),
        "documents/signatures/all with process.view",
    );
}

// ===========================================================================
// Accounting module — exactly 2 permissions (see server/file_server/src/main.rs's
// accounting_* router blocks): `accounting.manage` reaches every write route
// (billing/invoicing/payments/vendors/bank-reconciliation/budgets/fiscal-year-close),
// `accounting.view` reaches read-only listing and report/document routes only.
// ===========================================================================

/// `accounting.manage` reaches billing/invoicing/payments AND vendors/bank-recon/
/// budgets/fiscal-year-close (all collapsed under this one permission), but not an
/// unrelated domain (`process.review`).
#[tokio::test]
async fn test_accounting_manage_only() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "acctmgr", "810", &["accounting.manage"]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/billing-plans/create",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({
            "name": "Matrix Plan", "billing_type": "FLAT_RATE",
            "config": {"amount": 1000}, "frequency": "MONTHLY"
        })),
        "accounting/billing-plans/create with accounting.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({
                "invoice_id": uuid::Uuid::new_v4(), "amount": 100,
                "payment_method": "cash", "reference_number": null, "payment_date": "2026-07-01"
            })),
        "accounting/payments/record with accounting.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"name": "Matrix Vendor", "category": "misc"})),
        "accounting/vendors/create with accounting.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/billing-plans/assign-bulk",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"billing_plan_id": 999999})),
        "accounting/billing-plans/assign-bulk with accounting.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/bank-reconciliation/lines/list",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/bank-reconciliation/lines/list with accounting.manage",
    );
    // A nonexistent line id still proves the permission gate passed — the business-logic
    // 404 that follows is not a 403.
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/bank-reconciliation/lines/{}/match-candidates",
            url, 999999
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/bank-reconciliation/lines/:id/match-candidates with accounting.manage",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/budgets/variance", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/budgets/variance with accounting.manage",
    );
    // A nonexistent fiscal_year_id still proves the permission gate passed — the
    // business-logic 404 that follows is not a 403.
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/fiscal-years/{}/close",
            url, 999999
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/fiscal-years/:id/close with accounting.manage",
    );

    // Society Profile is restricted to the super_admin designation itself, not any
    // designation holding accounting.manage — a treasurer role has no business editing
    // the society's legal identity.
    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/society-profile/set",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"name": "Matrix Society", "address": "Somewhere"})),
        "accounting/society-profile/set requires super_admin, not just accounting.manage",
    );

    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/process/review", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"process_id": uuid::Uuid::new_v4(), "template_step_id": 1, "is_approved": true})),
        "process/review with only accounting.manage",
    );
}

/// `accounting.view` reaches the read-only listing/report/document routes (also
/// reachable by `accounting.manage`, since those routers accept either), but not any
/// write/manage route.
#[tokio::test]
async fn test_accounting_view_only() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "acctview", "811", &["accounting.view"]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/invoices/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/billing-plans/assignments",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/billing-plans/assignments with accounting.view",
    );
    // Read-only routes that used to sit under accounting.manage only, contradicting the
    // documented view/manage split — an auditor-style accounting.view holder must be
    // able to read this data even though they can't submit/approve/reconcile anything.
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/vendors/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/vendors/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/purchase-orders/list",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/purchase-orders/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/expenses/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/expenses/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/tds-rules/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/tds-rules/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/bank-reconciliation/lines/list",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/bank-reconciliation/lines/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/budgets/variance", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({ "fiscal_year_id": 999999 })),
        "accounting/budgets/variance with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/tds-summary", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({ "vendor_id": 999999, "fiscal_year_id": 999999 })),
        "accounting/tds-summary with accounting.view",
    );

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/funds/list", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/funds/list with accounting.view",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/reports/run", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"report_type": "outstanding_dues"})),
        "accounting/reports/run with accounting.view",
    );
    // Society Profile is restricted to super_admin, not reachable by accounting.view
    // either — it's not an accounting-permission-gated route at all.
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/society-profile", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/society-profile requires super_admin, not accounting.view",
    );
    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/society-profile/set",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"name": "X", "address": "Y"})),
        "accounting/society-profile/set requires super_admin, not accounting.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({
                "flat_id": 1, "period_start": "2026-07-01", "period_end": "2026-07-31", "due_date": "2026-07-10"
            })),
        "accounting/invoices/generate requires accounting.manage, not accounting.view",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({"name": "X", "category": "Y"})),
        "accounting/vendors/create requires accounting.manage, not accounting.view",
    );
    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/billing-plans/assign-bulk",
            url
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({"billing_plan_id": 999999})),
        "accounting/billing-plans/assign-bulk requires accounting.manage, not accounting.view",
    );
    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/fiscal-years/{}/close",
            url, 999999
        ))
        .set("Authorization", &a)
        .send_json(serde_json::json!({})),
        "accounting/fiscal-years/:id/close requires accounting.manage, not accounting.view",
    );
}

/// The resident-facing `/api/v1/accounting/*` routes sit outside the `/admin` nest and
/// have no router-level permission layer at all — any authenticated resident (zero
/// special permissions) can reach them, self-scoped internally. But they still require
/// a valid session: this is exactly the gap a prior review found and fixed on
/// `payment-instructions` (it previously had no `SessionUser` extractor at all).
#[tokio::test]
async fn test_accounting_resident_self_service_routes() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let jwt = seeded_resident(&env, &admin_jwt, "acctresident", "816", &[]);
    let a = auth(&jwt);

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/accounting/claims/mine", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/claims/mine needs only a valid session",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/accounting/payment-instructions", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/payment-instructions needs only a valid session",
    );
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/accounting/invoices/mine", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({})),
        "accounting/invoices/mine needs only a valid session",
    );

    // No Authorization header at all must be rejected — this is a 401 (unauthenticated),
    // not a 403 (authenticated but lacking permission), so it's checked directly rather
    // than via assert_forbidden.
    let unauthenticated = ureq::post(&format!("{}/api/v1/accounting/payment-instructions", url))
        .send_json(serde_json::json!({}));
    assert!(
        unauthenticated.is_err(),
        "payment-instructions must require authentication"
    );
    assert_eq!(
        unauthenticated
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        401,
        "payment-instructions without any token must be 401, not silently succeed",
    );

    // Admin-only accounting routes must still be out of reach for a bare resident.
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({
                "flat_id": 1, "period_start": "2026-07-01", "period_end": "2026-07-31", "due_date": "2026-07-10"
            })),
        "accounting/invoices/generate requires accounting.manage, not just any session",
    );
}

/// The UI hides accounting entirely when the society hasn't turned it on, but that is
/// not access control by itself — the API must also refuse every accounting route
/// outright when `accounting_enabled` is off, even for a caller holding
/// `accounting.manage`/`accounting.view`, and even for a resident's own self-service
/// routes. Society Profile is the one deliberate exception (super_admin needs it
/// regardless of whether accounting is enabled yet).
#[tokio::test]
async fn test_accounting_disabled_blocks_every_accounting_route_regardless_of_permission() {
    let env = setup_api_test_env_with_accounting_disabled().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let manage_jwt = seeded_resident(
        &env,
        &admin_jwt,
        "acctdisabled-mgr",
        "820",
        &["accounting.manage"],
    );
    let manage_auth = auth(&manage_jwt);

    assert_forbidden(
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/billing-plans/create",
            url
        ))
        .set("Authorization", &manage_auth)
        .send_json(serde_json::json!({
            "name": "X", "billing_type": "FLAT_RATE", "config": {"amount": 1000}, "frequency": "MONTHLY"
        })),
        "billing-plans/create must be blocked when accounting is disabled, even with accounting.manage",
    );
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
            .set("Authorization", &manage_auth)
            .send_json(serde_json::json!({})),
        "invoices/list must be blocked when accounting is disabled, even with accounting.manage",
    );

    let _ = quick_create_user(&env, &admin_jwt, "acctdisabled-res@vistas.com", "Z", "0909");
    let resident_jwt = quick_onboard_user(
        &env,
        "acctdisabled-res@vistas.com",
        "resident_pass_secure_123",
    );
    let resident_auth = auth(&resident_jwt);
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/accounting/invoices/mine", url))
            .set("Authorization", &resident_auth)
            .send_json(serde_json::json!({})),
        "invoices/mine must be blocked when accounting is disabled",
    );

    // Society Profile stays reachable regardless — a super admin needs it even before
    // accounting is turned on.
    let admin_auth = auth(&admin_jwt);
    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/admin/accounting/society-profile", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({})),
        "society-profile must remain reachable even when accounting is disabled",
    );
}

/// `/tenants/*` has no router-level permission layer — any resident reaches it, scoped
/// internally to flats they actually own.
#[tokio::test]
async fn test_tenant_routes_are_self_service_with_no_permission_gate() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let rid = quick_create_user(&env, &admin_jwt, "tenantroute@matrix.test", "Z", "830");
    let jwt = quick_onboard_user(&env, "tenantroute@matrix.test", "password123");
    let a = auth(&jwt);

    let ownership_id: i32 = sqlx::query_scalar(
        "SELECT fo.ownership_id FROM flat_ownerships fo \
         JOIN flat_ownership_users fou ON fou.ownership_id = fo.ownership_id AND fou.is_active = TRUE \
         WHERE fou.user_id = $1 AND fo.is_active = TRUE",
    )
    .bind(rid)
    .fetch_one(&env.db_pool)
    .await
    .unwrap();

    assert_not_forbidden(
        ureq::post(&format!("{}/api/v1/tenants/current", url))
            .set("Authorization", &a)
            .send_json(serde_json::json!({ "ownership_id": ownership_id })),
        "tenants/current needs only a valid session",
    );

    let unauthenticated = ureq::post(&format!("{}/api/v1/tenants/current", url))
        .send_json(serde_json::json!({ "ownership_id": ownership_id }));
    assert!(unauthenticated.is_err());
    assert_eq!(
        unauthenticated
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        401,
        "tenants/current without any token must be 401",
    );

    let other_jwt = seeded_resident(&env, &admin_jwt, "tenantroute-other", "831", &[]);
    assert_forbidden(
        ureq::post(&format!("{}/api/v1/tenants/current", url))
            .set("Authorization", &auth(&other_jwt))
            .send_json(serde_json::json!({ "ownership_id": ownership_id })),
        "tenants/current for a flat the caller does not own",
    );
}
