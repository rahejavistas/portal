//! Coverage for the admin bulk document actions (bulk-delete, bulk-update-subcategory,
//! bulk-update-category): itemized skip reporting, per-document category scoping (the
//! IDOR-style check — holding manage access on one category must never let a bulk request
//! reach into a category the caller can't manage), and the RESIDENT_SPECIFIC flat mapping.

use quality::{
    quick_assign_designation, quick_create_document, quick_create_document_category,
    quick_create_document_subcategory, quick_create_user, quick_onboard_user,
    quick_set_category_designation, quick_setup_admin, setup_api_test_env, TestEnv,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

/// Seeds a designation holding manage (read+manage) access to exactly one category, assigns
/// it to a fresh resident, and returns that resident's JWT.
fn seeded_category_manager(
    env: &TestEnv,
    admin_jwt: &str,
    category: &str,
    tag: &str,
    flat_number: &str,
) -> String {
    let url = env.server_url();
    let designation_code = format!("bulk_{}", tag);

    ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({"code": designation_code, "label": designation_code}))
        .expect("create designation");
    quick_set_category_designation(env, admin_jwt, category, &designation_code, true, true);

    let email = format!("{}@bulkdocs.test", tag);
    let rid = quick_create_user(env, admin_jwt, &email, "B", flat_number);
    let jwt = quick_onboard_user(env, &email, "password123");
    quick_assign_designation(env, admin_jwt, rid, &designation_code);
    jwt
}

fn get_document(env: &TestEnv, admin_jwt: &str, doc_id: &str) -> serde_json::Value {
    let url = env.server_url();
    ureq::post(&format!("{}/api/v1/admin/documents/{}/get", url, doc_id))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap()
}

/// The `/get` endpoint doesn't join flat info in; only `/list` does.
fn find_in_list(env: &TestEnv, admin_jwt: &str, doc_id: &str) -> serde_json::Value {
    let url = env.server_url();
    let docs: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/admin/documents/list", url))
        .set("Authorization", &auth(admin_jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    docs.into_iter()
        .find(|d| d["document"]["document_id"] == doc_id)
        .unwrap_or_else(|| panic!("document {} not found in list", doc_id))
}

/// A caller scoped to one category can bulk-delete a document in that category, but a
/// document in a category they can't manage is skipped (not deleted, not a 403 for the
/// whole batch) — this is the real per-document ID-scoping check, not just a route gate.
#[tokio::test]
async fn test_bulk_delete_skips_documents_caller_cannot_manage() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_DEL_A", "Bulk Del A");
    quick_create_document_category(&env, &admin_jwt, "BULK_DEL_B", "Bulk Del B");

    let doc_a = quick_create_document(&env, &admin_jwt, "Doc A", "BULK_DEL_A", None);
    let doc_b = quick_create_document(&env, &admin_jwt, "Doc B", "BULK_DEL_B", None);
    let missing_id = uuid::Uuid::new_v4().to_string();

    let manager_jwt = seeded_category_manager(&env, &admin_jwt, "BULK_DEL_A", "del1", "601");

    let result: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/bulk-delete", url))
            .set("Authorization", &auth(&manager_jwt))
            .send_json(serde_json::json!({"document_ids": [doc_a, doc_b, missing_id]}))
            .unwrap()
            .into_json()
            .unwrap();

    let succeeded = result["succeeded"].as_array().unwrap();
    assert_eq!(succeeded, &[serde_json::Value::String(doc_a.clone())]);

    let skipped = result["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "doc_b and the missing id must be skipped");
    let skipped_ids: Vec<&str> = skipped
        .iter()
        .map(|s| s["document_id"].as_str().unwrap())
        .collect();
    assert!(skipped_ids.contains(&doc_b.as_str()));
    assert!(skipped_ids.contains(&missing_id.as_str()));

    assert!(
        get_document(&env, &admin_jwt, &doc_a)["document"]["is_deleted"]
            .as_bool()
            .unwrap(),
        "doc_a must actually be soft-deleted"
    );
    assert!(
        !get_document(&env, &admin_jwt, &doc_b)["document"]["is_deleted"]
            .as_bool()
            .unwrap(),
        "doc_b must be untouched — caller couldn't manage BULK_DEL_B"
    );
}

/// Each document's subcategory code is resolved against its own category, so a selection
/// spanning categories works; a document whose category doesn't offer that code is skipped.
#[tokio::test]
async fn test_bulk_update_subcategory_resolves_per_document_category() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_SUB_A", "Bulk Sub A");
    quick_create_document_category(&env, &admin_jwt, "BULK_SUB_B", "Bulk Sub B");
    quick_create_document_category(&env, &admin_jwt, "BULK_SUB_C", "Bulk Sub C");
    quick_create_document_subcategory(&env, &admin_jwt, "BULK_SUB_A", "URGENT", "Urgent (A)");
    quick_create_document_subcategory(&env, &admin_jwt, "BULK_SUB_B", "URGENT", "Urgent (B)");
    // BULK_SUB_C intentionally has no "URGENT" subcategory.

    let doc_a = quick_create_document(&env, &admin_jwt, "Doc A", "BULK_SUB_A", None);
    let doc_b = quick_create_document(&env, &admin_jwt, "Doc B", "BULK_SUB_B", None);
    let doc_c = quick_create_document(&env, &admin_jwt, "Doc C", "BULK_SUB_C", None);

    let result: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-subcategory",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_a, doc_b, doc_c],
        "subcategory_code": "URGENT"
    }))
    .unwrap()
    .into_json()
    .unwrap();

    let succeeded = result["succeeded"].as_array().unwrap();
    assert_eq!(succeeded.len(), 2);
    let skipped = result["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["document_id"], doc_c);

    assert_eq!(
        get_document(&env, &admin_jwt, &doc_a)["metadata"]["subcategory_label"],
        "Urgent (A)"
    );
    assert_eq!(
        get_document(&env, &admin_jwt, &doc_b)["metadata"]["subcategory_label"],
        "Urgent (B)"
    );
    assert!(get_document(&env, &admin_jwt, &doc_c)["metadata"]["subcategory_label"].is_null());
}

/// Passing `subcategory_code: null` clears the subcategory on every manageable document.
#[tokio::test]
async fn test_bulk_update_subcategory_can_clear() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_CLR", "Bulk Clear");
    quick_create_document_subcategory(&env, &admin_jwt, "BULK_CLR", "TAG", "Tag");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "BULK_CLR", None);

    ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-subcategory",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({"document_ids": [doc_id.clone()], "subcategory_code": "TAG"}))
    .unwrap();
    assert_eq!(
        get_document(&env, &admin_jwt, &doc_id)["metadata"]["subcategory_label"],
        "Tag"
    );

    let result: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-subcategory",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({"document_ids": [doc_id.clone()], "subcategory_code": null}))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(result["succeeded"].as_array().unwrap().len(), 1);
    assert!(get_document(&env, &admin_jwt, &doc_id)["metadata"]["subcategory_label"].is_null());
}

/// Bulk-moving documents into RESIDENT_SPECIFIC requires a flat, applies it to every
/// document in the batch, and clears any prior subcategory.
#[tokio::test]
async fn test_bulk_update_category_into_resident_specific_requires_and_applies_flat() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_CAT_SRC", "Bulk Cat Src");
    quick_create_document_subcategory(&env, &admin_jwt, "BULK_CAT_SRC", "MISC", "Misc");
    quick_create_user(&env, &admin_jwt, "flatowner@bulkdocs.test", "C", "701");

    let doc_a = quick_create_document(&env, &admin_jwt, "Doc A", "BULK_CAT_SRC", None);
    let doc_b = quick_create_document(&env, &admin_jwt, "Doc B", "BULK_CAT_SRC", None);
    ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_a))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({
            "title": "Doc A",
            "description": null,
            "category": "BULK_CAT_SRC",
            "metadata": {"subcategory_code": "MISC", "tags": [], "document_date": "2026-05-20"}
        }))
        .unwrap();

    // Missing flat is rejected outright.
    let missing_flat_resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-category",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_a, doc_b],
        "category": "RESIDENT_SPECIFIC"
    }));
    assert!(
        missing_flat_resp.is_err(),
        "RESIDENT_SPECIFIC without a flat must be rejected"
    );

    let result: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-category",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_a, doc_b],
        "category": "RESIDENT_SPECIFIC",
        "flat": {"wing": "C", "flat_number": "701"}
    }))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(result["succeeded"].as_array().unwrap().len(), 2);

    for doc_id in [&doc_a, &doc_b] {
        let doc = find_in_list(&env, &admin_jwt, doc_id);
        assert_eq!(doc["document"]["category"], "RESIDENT_SPECIFIC");
        assert_eq!(doc["flat"]["wing"], "C");
        assert_eq!(doc["flat"]["flat_number"], "0701");
        assert!(
            doc["metadata"]["subcategory_label"].is_null(),
            "subcategory from the old category must be cleared on category change"
        );
    }
}

/// Bulk category change onto a nonexistent flat leaves the whole batch untouched — the
/// flat is validated once for the batch, unlike per-document permission skips.
#[tokio::test]
async fn test_bulk_update_category_rejects_unknown_flat_and_applies_nothing() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_CAT_NOFLAT", "Bulk Cat No Flat");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "BULK_CAT_NOFLAT", None);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-category",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_id.clone()],
        "category": "RESIDENT_SPECIFIC",
        "flat": {"wing": "Z", "flat_number": "9999"}
    }));
    assert!(resp.is_err(), "nonexistent flat must be rejected");

    assert_eq!(
        get_document(&env, &admin_jwt, &doc_id)["document"]["category"],
        "BULK_CAT_NOFLAT",
        "category must be untouched when the flat lookup fails"
    );
}

/// A flat paired with a non-RESIDENT_SPECIFIC target category is rejected outright — a raw
/// API call can't create an orphaned document_flats row on a category that isn't flat-scoped.
#[tokio::test]
async fn test_bulk_update_category_rejects_flat_on_non_resident_specific_target() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_CAT_PUB", "Bulk Cat Pub");
    quick_create_user(&env, &admin_jwt, "strayflat@bulkdocs.test", "D", "801");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "BULK_CAT_PUB", None);

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-category",
        url
    ))
    .set("Authorization", &auth(&admin_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_id.clone()],
        "category": "PUBLIC",
        "flat": {"wing": "D", "flat_number": "801"}
    }));
    assert!(
        resp.is_err(),
        "a flat mapping on a non-RESIDENT_SPECIFIC target must be rejected"
    );

    assert_eq!(
        get_document(&env, &admin_jwt, &doc_id)["document"]["category"],
        "BULK_CAT_PUB",
        "category must be untouched when the request is rejected"
    );
}

/// A caller who manages the source category but not the destination is rejected outright —
/// destination authorization isn't a snapshot decision, it gates the write like source does.
#[tokio::test]
async fn test_bulk_update_category_requires_manage_access_on_destination() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_DST_SRC", "Bulk Dst Src");
    quick_create_document_category(&env, &admin_jwt, "BULK_DST_TARGET", "Bulk Dst Target");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "BULK_DST_SRC", None);

    let manager_jwt = seeded_category_manager(&env, &admin_jwt, "BULK_DST_SRC", "dst1", "610");

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-category",
        url
    ))
    .set("Authorization", &auth(&manager_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_id.clone()],
        "category": "BULK_DST_TARGET"
    }));
    assert!(
        resp.is_err(),
        "manage access on source alone must not authorize a move into an unmanaged destination"
    );

    assert_eq!(
        get_document(&env, &admin_jwt, &doc_id)["document"]["category"],
        "BULK_DST_SRC",
        "category must be untouched when destination access is missing"
    );
}

/// A caller who manages both the source and destination categories can move a document
/// between them — the atomic destination check doesn't over-restrict the legitimate case.
#[tokio::test]
async fn test_bulk_update_category_succeeds_with_both_source_and_destination_access() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "BULK_BOTH_SRC", "Bulk Both Src");
    quick_create_document_category(&env, &admin_jwt, "BULK_BOTH_DST", "Bulk Both Dst");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "BULK_BOTH_SRC", None);

    let manager_jwt = seeded_category_manager(&env, &admin_jwt, "BULK_BOTH_SRC", "both1", "611");
    quick_set_category_designation(&env, &admin_jwt, "BULK_BOTH_DST", "bulk_both1", true, true);

    let result: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/bulk-update-category",
        url
    ))
    .set("Authorization", &auth(&manager_jwt))
    .send_json(serde_json::json!({
        "document_ids": [doc_id.clone()],
        "category": "BULK_BOTH_DST"
    }))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(result["succeeded"].as_array().unwrap().len(), 1);

    assert_eq!(
        get_document(&env, &admin_jwt, &doc_id)["document"]["category"],
        "BULK_BOTH_DST"
    );
}
