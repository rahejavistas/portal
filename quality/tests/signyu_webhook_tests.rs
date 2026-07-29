//! Coverage for `POST /webhooks/signyu` — HMAC-SHA256 signature verification per
//! SignYu's documented webhook contract (`X-SignSetu-Signature: sha256=<hex>` over the
//! raw request body). Deliberately run without a real/mocked SignYu backend: every
//! case here is decided before (or immediately after) signature verification, so no
//! network call to SignYu is ever made — see `setup_api_test_env_with_signyu_webhook_secret`.

use hmac::Mac;
use quality::setup_api_test_env_with_signyu_webhook_secret;

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn test_webhook_rejected_when_no_secret_configured() {
    // setup_api_test_env (not the _with_signyu_webhook_secret variant) never sets
    // SIGNYU_WEBHOOK_SECRET, so state.signyu_webhook_secret is None.
    let env = quality::setup_api_test_env(3320).await;
    let url = env.server_url();

    let body = br#"{"event":"document.completed","documentId":"doc_1"}"#;
    let resp = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .set("X-SignSetu-Signature", "sha256=irrelevant")
        .send_bytes(body);
    assert!(
        resp.is_err(),
        "a webhook must be refused outright when no secret is configured to verify it"
    );
}

#[tokio::test]
async fn test_webhook_rejected_when_signature_header_missing() {
    let env = setup_api_test_env_with_signyu_webhook_secret(3321, "whsec_test_secret").await;
    let url = env.server_url();

    let body = br#"{"event":"document.completed","documentId":"doc_1"}"#;
    let resp = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .send_bytes(body);
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 403),
        other => panic!("expected 403, got {:?}", other),
    }
}

#[tokio::test]
async fn test_webhook_rejected_when_signature_wrong() {
    let env = setup_api_test_env_with_signyu_webhook_secret(3322, "whsec_test_secret").await;
    let url = env.server_url();

    let body = br#"{"event":"document.completed","documentId":"doc_1"}"#;
    let resp = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .set(
            "X-SignSetu-Signature",
            "sha256=0000000000000000000000000000000000000000000000000000000000000000",
        )
        .send_bytes(body);
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 403),
        other => panic!("expected 403, got {:?}", other),
    }
}

#[tokio::test]
async fn test_webhook_rejected_when_body_tampered_after_signing() {
    let env = setup_api_test_env_with_signyu_webhook_secret(3323, "whsec_test_secret").await;
    let url = env.server_url();

    let signed_body = br#"{"event":"document.completed","documentId":"doc_1"}"#;
    let header = sign("whsec_test_secret", signed_body);
    // Send a DIFFERENT body than the one the signature covers — simulates a MITM or a
    // buggy proxy re-serializing the JSON in transit.
    let actually_sent_body = br#"{"event":"document.completed","documentId":"doc_evil"}"#;

    let resp = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .set("X-SignSetu-Signature", &header)
        .send_bytes(actually_sent_body);
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 403),
        other => panic!("expected 403, got {:?}", other),
    }
}

#[tokio::test]
async fn test_webhook_accepted_and_ignored_without_document_id() {
    let env = setup_api_test_env_with_signyu_webhook_secret(3324, "whsec_test_secret").await;
    let url = env.server_url();

    let body = br#"{"event":"document.completed"}"#;
    let header = sign("whsec_test_secret", body);

    let resp: serde_json::Value = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .set("X-SignSetu-Signature", &header)
        .send_bytes(body)
        .expect("correctly signed webhook with no documentId should still be accepted")
        .into_json()
        .unwrap();
    assert_eq!(resp["status"], "ignored");
}

#[tokio::test]
async fn test_webhook_with_valid_signature_reaches_reconcile_and_fails_cleanly_without_signyu_configured(
) {
    // SIGNYU_API_KEY is never set by this helper, so state.signyu_client is None —
    // proves the signature check passed (we got past it) and the handler then failed
    // for the RIGHT reason (no provider configured), not a signature error, without
    // ever making a real network call.
    let env = setup_api_test_env_with_signyu_webhook_secret(3325, "whsec_test_secret").await;
    let url = env.server_url();

    let body = br#"{"event":"document.completed","documentId":"doc_1"}"#;
    let header = sign("whsec_test_secret", body);

    let resp = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .set("X-SignSetu-Signature", &header)
        .send_bytes(body);
    match resp {
        Err(ureq::Error::Status(code, response)) => {
            assert_eq!(
                code, 500,
                "should fail at reconcile, not at signature verification"
            );
            let msg = response.into_string().unwrap_or_default();
            assert!(
                msg.contains("not configured"),
                "expected a 'SignYu is not configured' error, got: {}",
                msg
            );
        }
        other => panic!("expected 500 (SignYu not configured), got {:?}", other),
    }
}

#[tokio::test]
async fn test_webhook_rejects_malformed_json_after_valid_signature() {
    let env = setup_api_test_env_with_signyu_webhook_secret(3326, "whsec_test_secret").await;
    let url = env.server_url();

    let body = b"not valid json at all";
    let header = sign("whsec_test_secret", body);

    let resp = ureq::post(&format!("{}/webhooks/signyu", url))
        .set("Content-Type", "application/json")
        .set("X-SignSetu-Signature", &header)
        .send_bytes(body);
    match resp {
        Err(ureq::Error::Status(code, _)) => assert_eq!(code, 400),
        other => panic!("expected 400, got {:?}", other),
    }
}
