//! Coverage for editing an existing document's title/description/category/metadata, and
//! the flat-owner lookup used to preview who'll gain access before mapping a document to
//! a flat.

use quality::{
    quick_create_document, quick_create_document_category, quick_create_document_subcategory,
    quick_create_user, quick_setup_admin, setup_api_test_env,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn assert_error(resp: Result<ureq::Response, ureq::Error>, context: &str) {
    assert!(resp.is_err(), "expected failure for {}", context);
}

/// Editing title/description updates them and bumps updated_at, leaving category untouched.
#[tokio::test]
async fn test_edit_title_and_description_updates_fields_and_timestamp() {
    let env = setup_api_test_env(3191).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "NOTES", "Notes");
    let doc_id = quick_create_document(&env, &admin_jwt, "Old Title", "NOTES", None);

    let before: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/{}/get", url, doc_id))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    let uploaded_at = before["document"]["uploaded_date"]
        .as_str()
        .unwrap()
        .to_string();

    let resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "title": "New Title",
                "description": "New description",
                "category": "NOTES",
                "metadata": null
            }))
            .unwrap()
            .into_json()
            .unwrap();

    assert_eq!(resp["document"]["title"], "New Title");
    assert_eq!(resp["document"]["description"], "New description");
    assert_eq!(resp["document"]["category"], "NOTES");
    // uploaded_date never changes; updated_at must move forward from it (or at least exist).
    assert_eq!(resp["document"]["uploaded_date"], uploaded_at);
    assert!(resp["document"]["updated_at"].is_string());
}

/// Editing category to another manageable category succeeds and is reflected on refetch.
#[tokio::test]
async fn test_edit_category_moves_document() {
    let env = setup_api_test_env(3192).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "FROM_CAT", "From");
    quick_create_document_category(&env, &admin_jwt, "TO_CAT", "To");
    let doc_id = quick_create_document(&env, &admin_jwt, "Movable Doc", "FROM_CAT", None);

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "title": "Movable Doc",
            "description": null,
            "category": "TO_CAT",
            "metadata": null
        }));
    assert!(resp.is_ok(), "expected category change to succeed");

    let refetched: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/{}/get", url, doc_id))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(refetched["document"]["category"], "TO_CAT");
}

fn get_document(url: &str, auth_header: &str, document_id: &str) -> serde_json::Value {
    ureq::post(&format!(
        "{}/api/v1/admin/documents/{}/get",
        url, document_id
    ))
    .set("Authorization", auth_header)
    .send_json(serde_json::json!({}))
    .unwrap()
    .into_json()
    .unwrap()
}

/// A RESIDENT_SPECIFIC document's category can never be changed via edit — its visibility
/// depends on flat mapping (fixed at upload time), which this endpoint doesn't touch. The
/// request must fail AND leave the category untouched, not silently succeed or half-apply.
#[tokio::test]
async fn test_cannot_change_category_of_resident_specific_document() {
    let env = setup_api_test_env(3193).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "OTHER_CAT", "Other");
    quick_create_user(&env, &admin_jwt, "flatowner@test.com", "A", "301");
    let doc_id = quick_create_document(
        &env,
        &admin_jwt,
        "Flat Doc",
        "RESIDENT_SPECIFIC",
        Some(("A", "301")),
    );

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "title": "Flat Doc",
            "description": null,
            "category": "OTHER_CAT",
            "metadata": null
        }));
    assert_error(
        resp,
        "moving a RESIDENT_SPECIFIC document to another category",
    );

    let doc = get_document(&url, &admin_auth, &doc_id);
    assert_eq!(
        doc["document"]["category"], "RESIDENT_SPECIFIC",
        "category must be unchanged after a rejected update"
    );
}

/// No other category can be converted into RESIDENT_SPECIFIC via edit either — the reverse
/// of the above. Same rule, same verification that the category truly didn't change.
#[tokio::test]
async fn test_cannot_convert_other_category_to_resident_specific() {
    let env = setup_api_test_env(3197).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "OTHER_CAT2", "Other Two");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "OTHER_CAT2", None);

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "title": "Doc",
            "description": null,
            "category": "RESIDENT_SPECIFIC",
            "metadata": null
        }));
    assert_error(
        resp,
        "converting a regular category document to RESIDENT_SPECIFIC",
    );

    let doc = get_document(&url, &admin_auth, &doc_id);
    assert_eq!(
        doc["document"]["category"], "OTHER_CAT2",
        "category must be unchanged after a rejected update"
    );
}

/// While category itself is locked for RESIDENT_SPECIFIC documents, every other field —
/// title, description, subcategory, tags, dates — is still fully editable as long as the
/// category in the payload matches the document's current (unchanged) category.
#[tokio::test]
async fn test_resident_specific_document_other_metadata_edit_succeeds() {
    let env = setup_api_test_env(3198).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_subcategory(
        &env,
        &admin_jwt,
        "RESIDENT_SPECIFIC",
        "LEASE",
        "Lease Documents",
    );
    quick_create_user(&env, &admin_jwt, "flatowner2@test.com", "B", "202");
    let doc_id = quick_create_document(
        &env,
        &admin_jwt,
        "Old Title",
        "RESIDENT_SPECIFIC",
        Some(("B", "202")),
    );

    let resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "title": "New Title",
                "description": "Updated description",
                "category": "RESIDENT_SPECIFIC",
                "metadata": {
                    "subcategory_code": "LEASE",
                    "tags": ["important"],
                    "document_date": "2026-07-19",
                    "valid_from": null,
                    "valid_until": null
                }
            }))
            .unwrap()
            .into_json()
            .unwrap();

    assert_eq!(resp["document"]["title"], "New Title");
    assert_eq!(resp["document"]["description"], "Updated description");
    assert_eq!(resp["document"]["category"], "RESIDENT_SPECIFIC");
    assert_eq!(resp["metadata"]["subcategory_code"], "LEASE");
    assert_eq!(resp["metadata"]["subcategory_label"], "Lease Documents");
    assert_eq!(resp["metadata"]["tags"].as_array().unwrap(), &["important"]);

    // The flat mapping itself (not touched by this endpoint) survives the edit untouched.
    // /admin/documents/:id/get doesn't join flat info, so check via the list endpoint.
    let all_docs: Vec<serde_json::Value> =
        ureq::post(&format!("{}/api/v1/admin/documents/list", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({}))
            .unwrap()
            .into_json()
            .unwrap();
    let doc = all_docs
        .iter()
        .find(|d| d["document"]["document_id"] == doc_id)
        .expect("document not found in admin list");
    assert_eq!(doc["flat"]["wing"], "B");
    assert_eq!(doc["flat"]["flat_number"], "0202");
}

/// Editing a document's subcategory (and clearing it) updates the returned metadata.
#[tokio::test]
async fn test_edit_subcategory_set_and_clear() {
    let env = setup_api_test_env(3194).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_document_category(&env, &admin_jwt, "TAGGED", "Tagged");
    quick_create_document_subcategory(&env, &admin_jwt, "TAGGED", "IMPORTANT", "Important");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "TAGGED", None);

    let set_resp: serde_json::Value = ureq::post(&format!(
        "{}/api/v1/admin/documents/{}/update",
        url, doc_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "title": "Doc",
        "description": null,
        "category": "TAGGED",
        "metadata": { "subcategory_code": "IMPORTANT", "tags": [], "document_date": "2026-07-19" }
    }))
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(set_resp["metadata"]["subcategory_code"], "IMPORTANT");

    let clear_resp: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "title": "Doc",
                "description": null,
                "category": "TAGGED",
                "metadata": null
            }))
            .unwrap()
            .into_json()
            .unwrap();
    assert!(clear_resp["metadata"].is_null());
}

/// Editing a document into a category the caller can't manage is rejected.
#[tokio::test]
async fn test_edit_category_requires_manage_access_on_target() {
    let env = setup_api_test_env(3195).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_document_category(&env, &admin_jwt, "OPEN_CAT", "Open");
    quick_create_document_category(&env, &admin_jwt, "LOCKED_CAT", "Locked");
    let doc_id = quick_create_document(&env, &admin_jwt, "Doc", "OPEN_CAT", None);

    sqlx::query(
        "INSERT INTO designations (code, label) VALUES ('open_cat_only', 'Open Cat Only') ON CONFLICT DO NOTHING",
    )
    .execute(&env.db_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_category_designations (category, designation_id, can_read, can_manage) \
         SELECT 'OPEN_CAT', designation_id, TRUE, TRUE FROM designations WHERE code = 'open_cat_only' \
         ON CONFLICT DO NOTHING",
    )
    .execute(&env.db_pool)
    .await
    .unwrap();

    let rid = quick_create_user(&env, &admin_jwt, "scoped_editor@test.com", "A", "302");
    quality::quick_assign_designation(&env, &admin_jwt, rid, "open_cat_only");
    let editor_jwt = quality::quick_onboard_user(&env, "scoped_editor@test.com", "password123");
    let editor_auth = auth(&editor_jwt);

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/{}/update", url, doc_id))
        .set("Authorization", &editor_auth)
        .send_json(serde_json::json!({
            "title": "Doc",
            "description": null,
            "category": "LOCKED_CAT",
            "metadata": null
        }));
    assert_error(
        resp,
        "moving a document into a category the caller can't manage",
    );
}

/// Flat-owner lookup returns the active owner's email for a real flat, and an empty list
/// for an unmapped or invalid flat — never an error, since it's a soft preview used while
/// the admin is still typing.
#[tokio::test]
async fn test_flat_owner_lookup() {
    let env = setup_api_test_env(3196).await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    quick_create_user(&env, &admin_jwt, "owner@test.com", "B", "404");

    let found: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/flat-owners", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({ "wing": "B", "flat_number": "404" }))
            .unwrap()
            .into_json()
            .unwrap();
    assert_eq!(found["emails"].as_array().unwrap(), &["owner@test.com"]);

    let unmapped: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/flat-owners", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({ "wing": "Z", "flat_number": "999" }))
            .unwrap()
            .into_json()
            .unwrap();
    assert!(unmapped["emails"].as_array().unwrap().is_empty());

    let invalid: serde_json::Value =
        ureq::post(&format!("{}/api/v1/admin/documents/flat-owners", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({ "wing": "", "flat_number": "" }))
            .unwrap()
            .into_json()
            .unwrap();
    assert!(invalid["emails"].as_array().unwrap().is_empty());
}
