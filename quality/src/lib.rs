use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::process::{Child, Command, Stdio};
use std::sync::Once;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const TEST_POSTGRES_PORT: u16 = 5439;
pub const TEST_MINIO_PORT: u16 = 9009;
pub const TEST_MINIO_CONSOLE_PORT: u16 = 9010;

const POSTGRES_CONTAINER: &str = "vistas-test-postgres";
const MINIO_CONTAINER: &str = "vistas-test-minio";

static GLOBAL_INIT: Once = Once::new();

/// Isolated, drop-safe test environment.
pub struct TestEnv {
    pub db_name: String,
    pub db_pool: PgPool,
    pub server_process: Option<Child>,
    pub server_port: Option<u16>,
}

impl TestEnv {
    pub fn server_url(&self) -> String {
        format!(
            "http://localhost:{}",
            self.server_port.expect("No server port assigned")
        )
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        if let Some(mut child) = self.server_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        let db_name = self.db_name.clone();
        let db_url = format!(
            "postgres://postgres:password@localhost:{}/postgres",
            TEST_POSTGRES_PORT
        );

        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                if let Ok(mut conn) = PgConnection::connect(&db_url).await {
                    let terminate_query = format!(
                        "SELECT pg_terminate_backend(pg_stat_activity.pid) \
                         FROM pg_stat_activity \
                         WHERE pg_stat_activity.datname = '{}' AND pid <> pg_backend_pid()",
                        db_name
                    );
                    let _ = conn.execute(terminate_query.as_str()).await;
                    let _ = conn
                        .execute(format!("DROP DATABASE \"{}\"", db_name).as_str())
                        .await;
                }
            });
        })
        .join();

        let _ = std::fs::remove_file(format!("config_{}.toml", self.db_name));
    }
}

/// Ensures Postgres and Minio docker containers are running exactly once per test run.
pub fn ensure_global_containers_running() {
    GLOBAL_INIT.call_once(|| {
        // Check if postgres is already running and healthy
        let pg_running = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", POSTGRES_CONTAINER])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false);

        if !pg_running {
            let _ = Command::new("docker").args(["rm", "-f", POSTGRES_CONTAINER]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let pg_status = Command::new("docker")
                .args([
                    "run",
                    "--name", POSTGRES_CONTAINER,
                    "-p", &format!("{}:5432", TEST_POSTGRES_PORT),
                    "-e", "POSTGRES_USER=postgres",
                    "-e", "POSTGRES_PASSWORD=password",
                    "-e", "POSTGRES_DB=postgres",
                    "-d",
                    "postgres:15-alpine"
                ])
                .status()
                .expect("Failed to start Postgres container");

            if !pg_status.success() {
                panic!("Failed to launch Postgres container.");
            }
        }

        // Check if minio is already running
        let minio_running = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", MINIO_CONTAINER])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false);

        if !minio_running {
            let _ = Command::new("docker").args(["rm", "-f", MINIO_CONTAINER]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let minio_status = Command::new("docker")
                .args([
                    "run",
                    "--name", MINIO_CONTAINER,
                    "-p", &format!("{}:9000", TEST_MINIO_PORT),
                    "-p", &format!("{}:9001", TEST_MINIO_CONSOLE_PORT),
                    "-e", "MINIO_ROOT_USER=admin",
                    "-e", "MINIO_ROOT_PASSWORD=password",
                    "-d",
                    "minio/minio",
                    "server", "/data", "--console-address", ":9001"
                ])
                .status()
                .expect("Failed to start Minio container");

            if !minio_status.success() {
                panic!("Failed to launch Minio container.");
            }
        }

        // Wait for Postgres to be ready (whether we started it or it was already running)
        let start = Instant::now();
        let mut pg_ready = false;
        while start.elapsed() < Duration::from_secs(30) {
            let status = Command::new("docker")
                .args(["exec", POSTGRES_CONTAINER, "pg_isready", "-U", "postgres"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if let Ok(st) = status {
                if st.success() {
                    pg_ready = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        if !pg_ready {
            panic!("Timeout waiting for Postgres container.");
        }

        // Configure bucket using mc (it's idempotent, so safe to run even if already running)
        let mc_status = Command::new("docker")
            .args([
                "run",
                "--rm",
                "--link", &format!("{}:minio", MINIO_CONTAINER),
                "--entrypoint", "sh",
                "minio/mc",
                "-c",
                "until mc alias set myminio http://minio:9000 admin password; do sleep 0.5; done; mc mb myminio/vistas-files || true; mc anonymous set public myminio/vistas-files || true"
            ])
            .status()
            .expect("Failed to configure bucket using mc");

        if !mc_status.success() {
            panic!("Failed to configure Minio S3 bucket.");
        }
    });
}

/// Dynamic isolated environment builder for API testing.
pub async fn setup_api_test_env(server_port: u16) -> TestEnv {
    ensure_global_containers_running();

    let rand_id = Uuid::new_v4().simple().to_string();
    let db_name = format!("db_api_{}", &rand_id[..12]);

    let root_db_url = format!(
        "postgres://postgres:password@localhost:{}/postgres",
        TEST_POSTGRES_PORT
    );
    let mut conn = PgConnection::connect(&root_db_url)
        .await
        .expect("Failed to connect to root Postgres");

    conn.execute(format!("CREATE DATABASE \"{}\"", db_name).as_str())
        .await
        .expect("Failed to create dynamic database");

    let isolated_db_url = format!(
        "postgres://postgres:password@localhost:{}/{}",
        TEST_POSTGRES_PORT, db_name
    );
    let db_pool = PgPool::connect(&isolated_db_url)
        .await
        .expect("Failed to connect to dynamic database");

    infra::db::run_migrations(&db_pool)
        .await
        .expect("Failed to run migrations");

    let binary_path = format!(
        "../target/debug/file_server{}",
        std::env::consts::EXE_SUFFIX
    );
    let config_content = format!(
        r#"
port = {server_port}
admin_email = "admin@vistas.com"
jwt_secret = "modular_integration_test_secret_signing_key"
jwt_expiry = "1h"
forgot_password_expiry = "5m"

[postgres]
host = "localhost"
port = {TEST_POSTGRES_PORT}
username = "postgres"
password = "password"
database = "{db_name}"
max_connections = 5

[s3]
region = "us-east-1"
bucket_name = "vistas-files"
endpoint_url = "http://localhost:{TEST_MINIO_PORT}"
public_endpoint_url = "http://localhost:{TEST_MINIO_PORT}"
presigned_expiry = "15m"

[telemetry]

[rate_limit]
ip_per_min = 300
user_per_min = 120
admin_per_min = 300
"#
    );
    std::fs::write(format!("config_{}.toml", db_name), config_content)
        .expect("Failed to write dynamic test config");

    let server_child = Command::new(&binary_path)
        .env("APP_ENV", db_name.clone())
        .env("AWS_ACCESS_KEY_ID", "admin")
        .env("AWS_SECRET_ACCESS_KEY", "password")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn file_server");

    let health_url = format!("http://localhost:{}/health", server_port);
    let start_server = Instant::now();
    let mut server_ready = false;
    while start_server.elapsed() < Duration::from_secs(15) {
        if let Ok(resp) = ureq::get(&health_url).call() {
            if resp.status() == 200 {
                server_ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    if !server_ready {
        let mut child = server_child;
        let _ = child.kill();
        panic!(
            "Timeout waiting for Axum server to boot on port {}.",
            server_port
        );
    }

    TestEnv {
        db_name,
        db_pool,
        server_process: Some(server_child),
        server_port: Some(server_port),
    }
}

/// Dynamic isolated environment builder for database internal testing.
pub async fn setup_db_test_env() -> TestEnv {
    ensure_global_containers_running();

    let rand_id = Uuid::new_v4().simple().to_string();
    let db_name = format!("db_internal_{}", &rand_id[..12]);

    let root_db_url = format!(
        "postgres://postgres:password@localhost:{}/postgres",
        TEST_POSTGRES_PORT
    );
    let mut conn = PgConnection::connect(&root_db_url)
        .await
        .expect("Failed to connect to root Postgres");

    conn.execute(format!("CREATE DATABASE \"{}\"", db_name).as_str())
        .await
        .expect("Failed to create dynamic database");

    let isolated_db_url = format!(
        "postgres://postgres:password@localhost:{}/{}",
        TEST_POSTGRES_PORT, db_name
    );
    let db_pool = PgPool::connect(&isolated_db_url)
        .await
        .expect("Failed to connect to dynamic database");

    infra::db::run_migrations(&db_pool)
        .await
        .expect("Failed to run migrations");

    TestEnv {
        db_name,
        db_pool,
        server_process: None,
        server_port: None,
    }
}

/// Onboards the seeded admin user programmatically and returns the Admin JWT token.
pub fn quick_setup_admin(env: &TestEnv) -> String {
    let url = env.server_url();

    let _ = ureq::post(&format!("{}/api/v1/forgot-password", url))
        .send_json(serde_json::json!({ "email": "admin@vistas.com" }))
        .expect("Failed to call forgot-password");

    let options = (*env.db_pool.connect_options()).clone();
    let token = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let pool = sqlx::postgres::PgPoolOptions::new().connect_with(options).await.unwrap();
            let tok = sqlx::query_scalar::<_, String>(
                "SELECT reset_token FROM user_credentials WHERE user_id = (SELECT user_id FROM users WHERE email = $1)"
            )
            .bind("admin@vistas.com")
            .fetch_one(&pool)
            .await
            .expect("reset token not found in db");
            pool.close().await;
            tok
        })
    })
    .join()
    .unwrap();

    let _ = ureq::post(&format!("{}/api/v1/reset-password", url))
        .send_json(serde_json::json!({
            "token": token,
            "password": "admin_secure_password_123"
        }))
        .expect("Failed to reset password");

    let login_resp = ureq::post(&format!("{}/api/v1/login", url))
        .send_json(serde_json::json!({
            "email": "admin@vistas.com",
            "password": "admin_secure_password_123"
        }))
        .expect("Admin login failed");
    let login_val: serde_json::Value = login_resp.into_json().unwrap();
    login_val["token"]
        .as_str()
        .expect("token missing")
        .to_string()
}

/// Creates a new user in the system using the Admin JWT and returns their `user_id`.
pub fn quick_create_user(
    env: &TestEnv,
    admin_jwt: &str,
    email: &str,
    wing: &str,
    flat_number: &str,
) -> i32 {
    let url = env.server_url();

    let resp = ureq::post(&format!("{}/api/v1/admin/users/create", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(serde_json::json!({
            "email": email,
            "wing": wing,
            "flat_number": flat_number
        }))
        .expect("Failed to quick create user");

    let val: serde_json::Value = resp.into_json().unwrap();
    val["user_id"].as_i64().expect("user_id missing") as i32
}

/// Onboards an existing user (password setup) and returns their authenticated JWT token.
pub fn quick_onboard_user(env: &TestEnv, email: &str, password: &str) -> String {
    let url = env.server_url();

    let _ = ureq::post(&format!("{}/api/v1/forgot-password", url))
        .send_json(serde_json::json!({ "email": email }))
        .expect("Failed to trigger forgot password");

    let options = (*env.db_pool.connect_options()).clone();
    let email_lowercase = email.trim().to_lowercase();
    let token = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let pool = sqlx::postgres::PgPoolOptions::new().connect_with(options).await.unwrap();
            let tok = sqlx::query_scalar::<_, String>(
                "SELECT reset_token FROM user_credentials WHERE user_id = (SELECT user_id FROM users WHERE email = $1)"
            )
            .bind(&email_lowercase)
            .fetch_one(&pool)
            .await
            .expect("reset token not found in db");
            pool.close().await;
            tok
        })
    })
    .join()
    .unwrap();

    let _ = ureq::post(&format!("{}/api/v1/reset-password", url))
        .send_json(serde_json::json!({
            "token": token,
            "password": password
        }))
        .expect("Failed to reset password");

    let login_resp = ureq::post(&format!("{}/api/v1/login", url))
        .send_json(serde_json::json!({
            "email": email,
            "password": password
        }))
        .expect("Login failed");
    let login_val: serde_json::Value = login_resp.into_json().unwrap();
    login_val["token"].as_str().unwrap().to_string()
}

/// Promotes a user to the specified target role ('admin' or 'committee') using Admin JWT.
pub fn quick_promote_user(env: &TestEnv, admin_jwt: &str, user_id: i32, role: &str) {
    let url = env.server_url();
    let _ = ureq::post(&format!("{}/api/v1/admin/users/roles", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(serde_json::json!({
            "user_id": user_id,
            "target_role": role
        }))
        .expect("Failed to update role");
}

/// Programmatically creates a document via multipart upload using Admin JWT, returning the `document_id`.
pub fn quick_create_document(
    env: &TestEnv,
    admin_jwt: &str,
    title: &str,
    category: &str,
    doc_type: &str,
    access_level: &str,
    flat: Option<(&str, &str)>,
) -> String {
    let url = env.server_url();
    let boundary = "----TestBoundaryQuickHelperBoundary";
    let mock_bytes = b"Quick framework helper seeded file document bytes.";

    let mut payload = serde_json::json!({
        "title": title,
        "description": "Programmatic helper seeded document",
        "category": category,
        "document_type": doc_type,
        "access_level": access_level,
        "metadata": {
            "subcategory": "Seeded",
            "tags": ["seeded", "helper"],
            "document_date": "2026-05-20"
        }
    });

    if let Some((wing, flat_number)) = flat {
        payload["flat"] = serde_json::json!({
            "wing": wing,
            "flat_number": flat_number
        });
    }

    let body = create_multipart_body(
        boundary,
        &payload.to_string(),
        mock_bytes,
        "seeded.txt",
        "text/plain",
    );

    let resp = ureq::post(&format!("{}/api/v1/admin/documents/create", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .expect("Failed to upload document");

    let val: serde_json::Value = resp.into_json().unwrap();
    val["document"]["document_id"]
        .as_str()
        .expect("document_id missing")
        .to_string()
}

/// Helper function to construct a raw multipart/form-data body for uploading files via `ureq`
pub fn create_multipart_body(
    boundary: &str,
    payload_json: &str,
    file_bytes: &[u8],
    filename: &str,
    file_content_type: &str,
) -> Vec<u8> {
    let mut body = Vec::new();

    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"payload\"\r\n");
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(payload_json.as_bytes());
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file_content_type).as_bytes());
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    body
}
