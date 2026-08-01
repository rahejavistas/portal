//! Coverage for a single user owning multiple flats simultaneously — there's no DB
//! constraint limiting a user to one active flat ownership (only one active owner per
//! flat), so residents genuinely can and do own more than one unit. These tests prove
//! documents and processes behave correctly across both owned flats, and that revoking
//! one ownership doesn't collapse access the user still has through the other.

use quality::{
    quick_create_document, quick_create_user, quick_onboard_user, quick_setup_admin,
    setup_api_test_env,
};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

/// Self-uploads a RESIDENT_SPECIFIC document via the presigned-URL flow and returns
/// its document_id. Mirrors `quick_create_document` but goes through the resident-facing
/// `/api/v1/documents/*` endpoints (which auto-resolve the caller's owned flat(s))
/// instead of the admin path (which takes an explicit flat).
fn self_upload_document(url: &str, res_auth: &str, title: &str) -> String {
    let url_resp = ureq::post(&format!("{}/api/v1/documents/upload-url", url))
        .set("Authorization", res_auth)
        .send_json(serde_json::json!({
            "title": title,
            "filename": "personal.txt",
            "content_type": "text/plain"
        }))
        .expect("upload-url request failed");
    let val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = val["document_id"].as_str().unwrap().to_string();
    let upload_url = val["upload_url"].as_str().unwrap().to_string();

    ureq::put(&upload_url)
        .set("Content-Type", "text/plain")
        .send_bytes(b"personal document bytes")
        .expect("S3 PUT upload failed");

    let confirm_resp = ureq::post(&format!("{}/api/v1/documents/confirm-upload", url))
        .set("Authorization", res_auth)
        .send_json(serde_json::json!({ "document_id": document_id }))
        .expect("confirm-upload request failed");
    let confirm_val: serde_json::Value = confirm_resp.into_json().unwrap();
    assert_eq!(confirm_val["status"].as_str().unwrap(), "confirmed");

    document_id
}

/// A user owning two flats stays a single account (same user_id), and `/profile`
/// reports both flats aggregated (wing/flat_number as comma-joined lists).
#[tokio::test]
async fn test_owning_two_flats_is_a_single_account() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let rid1 = quick_create_user(&env, &admin_jwt, "twoflat@test.com", "A", "201");
    let rid2 = quick_create_user(&env, &admin_jwt, "twoflat@test.com", "A", "202");
    assert_eq!(rid1, rid2, "same email must resolve to the same user_id");

    let res_jwt = quick_onboard_user(&env, "twoflat@test.com", "password123");
    let profile: serde_json::Value = ureq::post(&format!("{}/api/v1/profile", url))
        .set("Authorization", &auth(&res_jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();

    assert!(profile["wing"].as_str().unwrap().contains('A'));
    assert!(profile["flat_number"].as_str().unwrap().contains("0201"));
    assert!(profile["flat_number"].as_str().unwrap().contains("0202"));
}

/// A resident self-uploading a RESIDENT_SPECIFIC document while owning two flats gets
/// a document linked to both — proving the fix for `get_active_flat_ids` (previously
/// `LIMIT 1`, which picked one arbitrary flat and silently orphaned the document if that
/// specific ownership was later revoked). The document must remain visible after either
/// single flat is revoked, since the other ownership still grants access.
#[tokio::test]
async fn test_self_uploaded_document_survives_revoking_one_of_two_flats() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let rid = quick_create_user(&env, &admin_jwt, "survive@test.com", "B", "301");
    quick_create_user(&env, &admin_jwt, "survive@test.com", "B", "302");
    let res_jwt = quick_onboard_user(&env, "survive@test.com", "password123");
    let res_auth = auth(&res_jwt);

    let doc_id = self_upload_document(&url, &res_auth, "My Personal Doc");

    // Visible right after upload.
    let get_resp = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(get_resp.is_ok(), "document should be visible after upload");

    // Revoke ONE of the two flats.
    let revoke_resp = ureq::post(&format!("{}/api/v1/admin/users/revoke-flat", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"user_id": rid, "wing": "B", "flat_number": "301"}));
    assert!(revoke_resp.is_ok(), "revoking one flat should succeed");

    // The document is still linked to the other (B-302) flat, so still visible.
    let after_resp = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(
        after_resp.is_ok(),
        "document should remain visible via the still-owned flat after revoking the other"
    );

    let list_resp: Vec<serde_json::Value> = ureq::post(&format!("{}/api/v1/documents", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(list_resp
        .iter()
        .any(|d| d["document"]["document_id"].as_str() == Some(doc_id.as_str())));
}

/// An admin-uploaded, flat-scoped document targeted at ONE of a user's two flats is
/// visible to that user; once that specific flat's ownership is revoked, that document
/// (unlike the self-uploaded one above, which was linked to both) is no longer visible —
/// while a second document targeted at the still-owned flat remains visible throughout.
#[tokio::test]
async fn test_admin_flat_targeted_documents_respect_each_flat_independently() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let rid = quick_create_user(&env, &admin_jwt, "targeted@test.com", "C", "401");
    quick_create_user(&env, &admin_jwt, "targeted@test.com", "C", "402");
    let res_jwt = quick_onboard_user(&env, "targeted@test.com", "password123");
    let res_auth = auth(&res_jwt);

    let doc_flat1 = quick_create_document(
        &env,
        &admin_jwt,
        "Notice for C-401",
        "RESIDENT_SPECIFIC",
        Some(("C", "401")),
    );
    let doc_flat2 = quick_create_document(
        &env,
        &admin_jwt,
        "Notice for C-402",
        "RESIDENT_SPECIFIC",
        Some(("C", "402")),
    );

    for doc_id in [&doc_flat1, &doc_flat2] {
        let resp = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_id))
            .set("Authorization", &res_auth)
            .send_json(serde_json::json!({}));
        assert!(resp.is_ok(), "both flat-targeted docs visible up front");
    }

    let revoke_resp = ureq::post(&format!("{}/api/v1/admin/users/revoke-flat", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"user_id": rid, "wing": "C", "flat_number": "401"}));
    assert!(revoke_resp.is_ok());

    let after_flat1 = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_flat1))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(
        after_flat1.is_err(),
        "doc targeted at the revoked flat should no longer be visible"
    );

    let after_flat2 = ureq::post(&format!("{}/api/v1/documents/{}", url, doc_flat2))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}));
    assert!(
        after_flat2.is_ok(),
        "doc targeted at the still-owned flat should remain visible"
    );
}

/// A user with two active flat ownerships must disambiguate which flat a new process is
/// for — `/process/state` without `flat_id` returns 400 asking for one — and each flat
/// gets its own independent process instance, both listed under `/process/list` alongside
/// both owned flats.
#[tokio::test]
async fn test_multi_flat_user_gets_independent_process_per_flat() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    quick_create_user(&env, &admin_jwt, "twoproc@test.com", "D", "501");
    quick_create_user(&env, &admin_jwt, "twoproc@test.com", "D", "502");
    let res_jwt = quick_onboard_user(&env, "twoproc@test.com", "password123");
    let res_auth = auth(&res_jwt);

    // Ambiguous without flat_id: must be rejected, not silently pick one.
    let ambiguous = ureq::post(&format!("{}/api/v1/process/state", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({"template_id": 1}));
    match ambiguous {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!(
            "expected 400 for ambiguous multi-flat process init, got {:?}",
            other
        ),
    }

    let list_before: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let flats = list_before["flats"].as_array().unwrap();
    assert_eq!(flats.len(), 2, "both owned flats should be reported");
    let flat_ids: Vec<i64> = flats
        .iter()
        .map(|f| f["flat_id"].as_i64().unwrap())
        .collect();

    // Init one process per flat_id.
    let mut process_ids = Vec::new();
    for flat_id in &flat_ids {
        let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
            .set("Authorization", &res_auth)
            .send_json(serde_json::json!({"template_id": 1, "flat_id": flat_id}))
            .unwrap()
            .into_json()
            .unwrap();
        process_ids.push(resp["process_id"].as_str().unwrap().to_string());
    }
    assert_ne!(
        process_ids[0], process_ids[1],
        "each flat should get its own distinct process instance"
    );

    let list_after: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let processes = list_after["processes"].as_array().unwrap();
    assert_eq!(processes.len(), 2, "both processes should be listed");
    for pid in &process_ids {
        assert!(processes
            .iter()
            .any(|p| p["process_id"].as_str() == Some(pid.as_str())));
    }
}

/// Revoking one of a multi-flat user's ownerships hides only the process tied to that
/// specific ownership from `/process/list`; the process tied to the still-owned flat
/// (and that flat itself) remain listed.
#[tokio::test]
async fn test_revoking_one_flat_hides_only_that_flats_process() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);

    let rid = quick_create_user(&env, &admin_jwt, "revokeproc@test.com", "E", "601");
    quick_create_user(&env, &admin_jwt, "revokeproc@test.com", "E", "602");
    let res_jwt = quick_onboard_user(&env, "revokeproc@test.com", "password123");
    let res_auth = auth(&res_jwt);

    let list: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let flats = list["flats"].as_array().unwrap().clone();
    let flat_601 = flats
        .iter()
        .find(|f| f["flat_number"].as_str() == Some("0601"))
        .unwrap()["flat_id"]
        .as_i64()
        .unwrap();
    let flat_602 = flats
        .iter()
        .find(|f| f["flat_number"].as_str() == Some("0602"))
        .unwrap()["flat_id"]
        .as_i64()
        .unwrap();

    let mut pids = std::collections::HashMap::new();
    for (label, flat_id) in [("601", flat_601), ("602", flat_602)] {
        let resp: serde_json::Value = ureq::post(&format!("{}/api/v1/process/state", url))
            .set("Authorization", &res_auth)
            .send_json(serde_json::json!({"template_id": 1, "flat_id": flat_id}))
            .unwrap()
            .into_json()
            .unwrap();
        pids.insert(label, resp["process_id"].as_str().unwrap().to_string());
    }

    let revoke_resp = ureq::post(&format!("{}/api/v1/admin/users/revoke-flat", url))
        .set("Authorization", &auth(&admin_jwt))
        .send_json(serde_json::json!({"user_id": rid, "wing": "E", "flat_number": "601"}));
    assert!(revoke_resp.is_ok());

    let list_after: serde_json::Value = ureq::post(&format!("{}/api/v1/process/list", url))
        .set("Authorization", &res_auth)
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    let flats_after = list_after["flats"].as_array().unwrap();
    assert_eq!(flats_after.len(), 1, "only the still-owned flat remains");
    assert_eq!(flats_after[0]["flat_number"].as_str(), Some("0602"));

    let processes_after = list_after["processes"].as_array().unwrap();
    assert!(
        !processes_after
            .iter()
            .any(|p| p["process_id"].as_str() == Some(pids["601"].as_str())),
        "process tied to the revoked flat should disappear from the list"
    );
    assert!(
        processes_after
            .iter()
            .any(|p| p["process_id"].as_str() == Some(pids["602"].as_str())),
        "process tied to the still-owned flat should remain listed"
    );
}
