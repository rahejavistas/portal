//! Exercises `SignYuClient` against a real local HTTP server that speaks the documented
//! SignYu API shapes — both the client and the mock run in this same test process (no
//! subprocess, no cross-process networking), so this only depends on loopback HTTP and
//! a Postgres connection for the audit-log assertions.
//!
//! Covers both the happy path (`test_create_and_send_*`, `test_get_document_*`,
//! `test_download_*`) and SignYu failure modes (`test_*_returns_error_on_*`,
//! `test_*_fails_on_malformed_*`, `test_*_transport_failure_*`) — this is a paid
//! external API, so a non-2xx/malformed/unreachable response must fail loudly and
//! predictably rather than silently succeed with garbage data.

use axum::{
    extract::Path, extract::State, http::StatusCode, routing::get, routing::post, Json, Router,
};
use file_server::signature_providers::{signyu::SignYuClient, SignerContact};
use sqlx::{Connection, PgPool};
use std::net::SocketAddr;

async fn mock_create_document() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "documentId": "doc_mock_1", "name": "mock", "status": "PENDING" }))
}

async fn mock_add_signers(
    Path(_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let signers = body["signers"].as_array().cloned().unwrap_or_default();
    let out: Vec<serde_json::Value> = signers
        .iter()
        .enumerate()
        .map(|(i, _)| serde_json::json!({ "signerId": format!("signer_mock_{}", i + 1) }))
        .collect();
    Json(serde_json::json!({ "signers": out }))
}

async fn mock_send(Path(id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "documentId": id,
        "status": "SENT",
        "creditsRemaining": 48,
        "signers": [
            { "signerId": "signer_mock_1", "signUrl": "https://signyu.com/sign?sid=signer_mock_1" },
            { "signerId": "signer_mock_2", "signUrl": "https://signyu.com/sign?sid=signer_mock_2" }
        ]
    }))
}

async fn mock_get_document(
    Path(id): Path<String>,
    State(host_base_url): State<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "documentId": id,
        "status": "COMPLETED",
        "downloadUrl": format!("{}/signed.pdf", host_base_url),
        "signers": [{ "signerId": "signer_mock_1", "hasSigned": true }]
    }))
}

async fn mock_download_pdf() -> Vec<u8> {
    b"fully signed pdf bytes from signyu mock".to_vec()
}

async fn spawn_mock_signyu() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let host_base_url = format!("http://{}", addr);

    let app = Router::new()
        .route("/api/v1/documents", post(mock_create_document))
        .route("/api/v1/documents/:id/signers", post(mock_add_signers))
        .route("/api/v1/documents/:id/send", post(mock_send))
        .route("/api/v1/documents/:id", get(mock_get_document))
        .route("/signed.pdf", get(mock_download_pdf))
        .with_state(host_base_url.clone());

    tokio::spawn(async move {
        axum::Server::from_tcp(listener)
            .unwrap()
            .serve(app.into_make_service())
            .await
            .unwrap();
    });
    format!("{}/api/v1", host_base_url)
}

/// A standalone Postgres connection just for the `signature_provider_requests` log
/// table (no FK to the rest of the schema — this test only exercises the client, not
/// the full app, so `requirement_id` is left unconstrained here).
async fn test_pg_pool() -> PgPool {
    quality::ensure_global_containers_running();
    let root_url = "postgres://postgres:password@localhost:5439/postgres";
    let mut conn = sqlx::PgConnection::connect(root_url)
        .await
        .expect("connect to test postgres (is the quality test container running?)");
    let db_name = format!("db_signyu_client_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE \"{}\"", db_name))
        .execute(&mut conn)
        .await
        .expect("create test database");

    let db_url = format!("postgres://postgres:password@localhost:5439/{}", db_name);
    let pool = PgPool::connect(&db_url)
        .await
        .expect("connect to test database");

    sqlx::query(
        "CREATE TABLE signature_provider_requests ( \
            log_id SERIAL PRIMARY KEY, \
            provider VARCHAR(32) NOT NULL, \
            requirement_id INT, \
            direction VARCHAR(16) NOT NULL, \
            endpoint VARCHAR(255) NOT NULL, \
            method VARCHAR(8) NOT NULL, \
            request_body JSONB, \
            response_status INT, \
            response_body JSONB, \
            error TEXT, \
            created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW() \
        )",
    )
    .execute(&pool)
    .await
    .expect("create signature_provider_requests table");

    pool
}

#[tokio::test]
async fn test_create_and_send_returns_ids_in_order_and_logs_every_call() {
    let base_url = spawn_mock_signyu().await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool.clone());

    let signers = vec![
        SignerContact {
            name: "Asha".to_string(),
            phone: "9876543210".to_string(),
            email: "asha@example.com".to_string(),
        },
        SignerContact {
            name: "Vikram".to_string(),
            phone: "9812345678".to_string(),
            email: "vikram@example.com".to_string(),
        },
    ];

    let result = client
        .create_and_send(b"pdf bytes".to_vec(), "Test Document", &signers, None)
        .await
        .expect("create_and_send should succeed against the mock");

    assert_eq!(result.external_document_id, "doc_mock_1");
    assert_eq!(
        result.external_signer_ids,
        vec!["signer_mock_1", "signer_mock_2"]
    );
    assert_eq!(
        result.sign_urls,
        vec![
            Some("https://signyu.com/sign?sid=signer_mock_1".to_string()),
            Some("https://signyu.com/sign?sid=signer_mock_2".to_string())
        ]
    );

    let logged: Vec<(String, String, i32)> = sqlx::query_as(
        "SELECT endpoint, method, response_status FROM signature_provider_requests ORDER BY log_id",
    )
    .fetch_all(&pg_pool)
    .await
    .unwrap()
    .into_iter()
    .map(|(e, m, s): (String, String, Option<i32>)| (e, m, s.unwrap_or(0)))
    .collect();

    assert_eq!(
        logged.len(),
        3,
        "create_document + add_signers + send must each be logged"
    );
    assert!(logged
        .iter()
        .all(|(_, _, status)| *status == 200 || *status == 201));
}

#[tokio::test]
async fn test_get_document_parses_completed_status_and_signer_progress() {
    let base_url = spawn_mock_signyu().await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool);

    let status = client
        .get_document("doc_mock_1", None)
        .await
        .expect("get_document should succeed against the mock");

    assert!(status.is_completed);
    assert!(status.download_url.is_some());
    assert_eq!(status.signer_progress.len(), 1);
    assert_eq!(
        status.signer_progress[0].external_signer_id,
        "signer_mock_1"
    );
    assert!(status.signer_progress[0].has_signed);
}

#[tokio::test]
async fn test_download_signed_pdf_fetches_bytes() {
    let base_url = spawn_mock_signyu().await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url.clone(), "sk_test_fake".to_string(), pg_pool);

    let status = client.get_document("doc_mock_1", None).await.unwrap();
    let download_url = status.download_url.unwrap();

    let bytes = client
        .download_signed_pdf(&download_url)
        .await
        .expect("download_signed_pdf should succeed");
    assert_eq!(bytes, b"fully signed pdf bytes from signyu mock".to_vec());
}

// ==========================================================================
// Failure paths — SignYu is a paid external API, so every non-2xx, malformed, or
// unreachable response must be a clear `Err`, never a silent success with garbage data.
// ==========================================================================

/// A one-route mock returning a fixed status + JSON body for every request, regardless
/// of method/path — enough to simulate "SignYu returned an error" for any single call
/// without needing a full multi-route mock like `spawn_mock_signyu`.
async fn spawn_fixed_response_mock(status: StatusCode, body: serde_json::Value) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    let app = Router::new().fallback(move || async move { (status, Json(body)) });

    tokio::spawn(async move {
        axum::Server::from_tcp(listener)
            .unwrap()
            .serve(app.into_make_service())
            .await
            .unwrap();
    });
    format!("http://{}/api/v1", addr)
}

#[tokio::test]
async fn test_create_document_returns_error_on_non_2xx() {
    let base_url = spawn_fixed_response_mock(
        StatusCode::PAYMENT_REQUIRED,
        serde_json::json!({ "error": "insufficient credits" }),
    )
    .await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool.clone());

    let err = client
        .create_document(b"pdf bytes".to_vec(), "Test Document", None)
        .await
        .expect_err("a 402 from SignYu must surface as an error, not a fake success");
    assert!(
        err.contains("402"),
        "error should mention the status code: {}",
        err
    );

    let logged_status: Option<i32> =
        sqlx::query_scalar("SELECT response_status FROM signature_provider_requests LIMIT 1")
            .fetch_one(&pg_pool)
            .await
            .unwrap();
    assert_eq!(
        logged_status,
        Some(402),
        "the failing response must still be logged to signature_provider_requests"
    );
}

#[tokio::test]
async fn test_create_document_fails_on_missing_document_id() {
    let base_url = spawn_fixed_response_mock(
        StatusCode::OK,
        serde_json::json!({ "status": "PENDING" }), // no documentId field at all
    )
    .await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool);

    let err = client
        .create_document(b"pdf bytes".to_vec(), "Test Document", None)
        .await
        .expect_err("a 200 with no documentId must still be treated as a failure");
    assert!(
        err.contains("documentId"),
        "error should name the missing field: {}",
        err
    );
}

#[tokio::test]
async fn test_add_signers_fails_when_returned_ids_dont_match_requested_count() {
    // SignYu claims success but only returns one signerId for two requested signers —
    // must not silently proceed as if both were registered.
    let base_url = spawn_fixed_response_mock(
        StatusCode::OK,
        serde_json::json!({ "signers": [{ "signerId": "signer_mock_1" }] }),
    )
    .await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool);

    let signers = vec![
        SignerContact {
            name: "Asha".to_string(),
            phone: "9876543210".to_string(),
            email: "asha@example.com".to_string(),
        },
        SignerContact {
            name: "Vikram".to_string(),
            phone: "9812345678".to_string(),
            email: "vikram@example.com".to_string(),
        },
    ];

    let err = client
        .add_signers("doc_1", &signers, None)
        .await
        .expect_err("a signer-count mismatch must be rejected, not partially accepted");
    assert!(
        err.contains('1') && err.contains('2'),
        "error should mention both counts: {}",
        err
    );
}

#[tokio::test]
async fn test_send_returns_error_on_non_200() {
    let base_url = spawn_fixed_response_mock(
        StatusCode::INTERNAL_SERVER_ERROR,
        serde_json::json!({ "error": "internal error" }),
    )
    .await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool);

    let err = client
        .send("doc_1", None)
        .await
        .expect_err("a 500 from send() must surface as an error");
    assert!(
        err.contains("500"),
        "error should mention the status code: {}",
        err
    );
}

#[tokio::test]
async fn test_get_document_returns_error_on_non_200() {
    let base_url = spawn_fixed_response_mock(
        StatusCode::NOT_FOUND,
        serde_json::json!({ "error": "document not found" }),
    )
    .await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool);

    let err = client
        .get_document("doc_unknown", None)
        .await
        .expect_err("a 404 from get_document must surface as an error");
    assert!(
        err.contains("404"),
        "error should mention the status code: {}",
        err
    );
}

#[tokio::test]
async fn test_get_document_handles_malformed_body_without_crashing() {
    // 200 OK but none of the fields we normally parse are present.
    let base_url =
        spawn_fixed_response_mock(StatusCode::OK, serde_json::json!({ "unexpected": true })).await;
    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool);

    let status = client
        .get_document("doc_1", None)
        .await
        .expect("a 200 with an unexpected shape should degrade gracefully, not error");
    assert!(
        !status.is_completed,
        "missing status field must default to not-completed, never a false COMPLETED"
    );
    assert!(status.download_url.is_none());
    assert!(status.signer_progress.is_empty());
}

#[tokio::test]
async fn test_create_document_transport_failure_is_logged_and_returns_error() {
    // Bind and immediately drop the listener so the port is guaranteed to refuse
    // connections — a stand-in for "SignYu is unreachable" (DNS/timeout/connection
    // refused) without depending on outbound network access from this sandbox.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let base_url = format!("http://{}/api/v1", addr);

    let pg_pool = test_pg_pool().await;
    let client = SignYuClient::new(base_url, "sk_test_fake".to_string(), pg_pool.clone());

    let err = client
        .create_document(b"pdf bytes".to_vec(), "Test Document", None)
        .await
        .expect_err("an unreachable SignYu must surface as an error");
    assert!(
        err.contains("create_document"),
        "error should identify which operation failed: {}",
        err
    );

    let (logged_status, logged_error): (Option<i32>, Option<String>) =
        sqlx::query_as("SELECT response_status, error FROM signature_provider_requests LIMIT 1")
            .fetch_one(&pg_pool)
            .await
            .unwrap();
    assert_eq!(
        logged_status, None,
        "a transport-level failure has no HTTP status to log"
    );
    assert!(
        logged_error.is_some(),
        "a transport-level failure must still be logged with its error message"
    );
}
