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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to configure bucket using mc");

        if !mc_status.success() {
            panic!("Failed to configure Minio S3 bucket.");
        }
    });
}

/// The one and only place this fixture's JSON lives — real templates are DB-backed
/// (authored via the admin builder), so this exists purely for tests: both this
/// crate's own DB/API-level tests and, via a dev-dependency on this crate, `process`'s
/// own pure state-machine unit tests, which need a template with realistic shape.
pub fn load_test_template_fixture() -> process::ProcessTemplate {
    let content = include_str!("../fixtures/tenant_onboarding.json");
    serde_json::from_str(content).expect("Failed to parse test template fixture")
}

/// Every API-level test starts processes against the real `/api/v1/process/state`
/// endpoint, which requires an actual, ACTIVE `process_templates` row — there is no
/// production seed migration for this (it's meant to be authored via the admin
/// builder), so each fresh test DB needs its own. Seeds the fixture above, active
/// immediately. Since each test DB is freshly created, this deterministically
/// becomes template_id 1.
async fn seed_default_process_template(pool: &PgPool) {
    let template = load_test_template_fixture();

    let mut conn = pool.acquire().await.expect("Failed to acquire connection");

    let template_id: i32 = sqlx::query_scalar(
        "INSERT INTO process_templates (name) VALUES ($1) RETURNING template_id",
    )
    .bind(&template.name)
    .fetch_one(&mut *conn)
    .await
    .expect("Failed to seed process_templates");

    let definition = serde_json::to_value(&template).expect("Failed to serialize test template");
    let version_id: i32 = sqlx::query_scalar(
        "INSERT INTO process_template_versions (template_id, version_number, definition) \
         VALUES ($1, 1, $2) RETURNING version_id",
    )
    .bind(template_id)
    .bind(&definition)
    .fetch_one(&mut *conn)
    .await
    .expect("Failed to seed process_template_versions");

    sqlx::query("UPDATE process_templates SET active_version_id = $1 WHERE template_id = $2")
        .bind(version_id)
        .bind(template_id)
        .execute(&mut *conn)
        .await
        .expect("Failed to activate test template");
}

/// Dynamic isolated environment builder for API testing.
pub async fn setup_api_test_env() -> TestEnv {
    setup_api_test_env_inner(&[]).await
}

/// Same as `setup_api_test_env`, but with accounting turned OFF — module-gated
/// accounting endpoints are rejected regardless of permissions; Society Profile
/// stays reachable since it's exempt (needed to turn accounting on).
pub async fn setup_api_test_env_with_accounting_disabled() -> TestEnv {
    setup_api_test_env_inner(&[("ACCOUNTING_ENABLED", "false")]).await
}

/// Same as `setup_api_test_env`, but with the SignYu provider pointed at a local mock
/// server (`signyu_base_url`, e.g. `http://127.0.0.1:PORT`) instead of the real API —
/// lets tests exercise the SignYu request/response path against canned responses.
pub async fn setup_api_test_env_with_signyu(signyu_base_url: &str) -> TestEnv {
    setup_api_test_env_inner(&[
        ("SIGNYU_API_KEY", "sk_test_fake_key"),
        ("SIGNYU_BASE_URL", signyu_base_url),
    ])
    .await
}

/// Same as `setup_api_test_env`, but with `SIGNYU_WEBHOOK_SECRET` set — for testing
/// `POST /webhooks/signyu` signature verification. Deliberately does NOT set
/// `SIGNYU_API_KEY` (no `signyu_client` configured), so a webhook whose signature
/// verifies successfully still fails fast at the "reconcile" stage with a clear
/// "SignYu is not configured" error instead of attempting a real network call — safe
/// to run without a live or mocked SignYu backend.
pub async fn setup_api_test_env_with_signyu_webhook_secret(webhook_secret: &str) -> TestEnv {
    setup_api_test_env_inner(&[("SIGNYU_WEBHOOK_SECRET", webhook_secret)]).await
}

/// Asks the OS for a free TCP port instead of relying on every test to hand-pick a
/// unique number — tests used to each hardcode their own port literal, and it was only
/// a matter of time before two picked the same one (which happened) and started
/// intermittently failing whenever both ran at once. Binding to port 0 and reading back
/// whatever the OS assigned, then releasing it immediately before the child process
/// binds the same port itself, has a theoretical TOCTOU race but is the standard
/// approach and is effectively immune to the collisions manual numbering caused.
fn allocate_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("Failed to bind to an ephemeral port")
        .local_addr()
        .expect("Failed to read back the allocated port")
        .port()
}

async fn setup_api_test_env_inner(extra_env: &[(&str, &str)]) -> TestEnv {
    ensure_global_containers_running();
    let server_port = allocate_free_port();

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

    seed_default_process_template(&db_pool).await;

    let binary_path = format!(
        "../target/debug/file_server{}",
        std::env::consts::EXE_SUFFIX
    );
    let config_content = format!(
        r#"
port = {server_port}
super_admin_email = "superadmin@vistas.com"
jwt_secret = "modular_integration_test_secret_signing_key"
jwt_expiry = "1h"
forgot_password_expiry = "5m"
accounting_enabled = true

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
        .envs(extra_env.iter().map(|(k, v)| (*k, *v)))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
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
        .send_json(serde_json::json!({ "email": "superadmin@vistas.com" }))
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
            .bind("superadmin@vistas.com")
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
            "email": "superadmin@vistas.com",
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

/// Creates a new designation via the roles-management API, using the caller's JWT (which
/// must hold `designations.manage`). Designations are no longer seeded (beyond super_admin),
/// so tests must create any designation they need before assigning/mapping it.
pub fn quick_create_designation(env: &TestEnv, caller_jwt: &str, code: &str, label: &str) {
    let url = env.server_url();
    let _ = ureq::post(&format!("{}/api/v1/admin/roles/designations/create", url))
        .set("Authorization", &format!("Bearer {}", caller_jwt))
        .send_json(serde_json::json!({ "code": code, "label": label }))
        .expect("Failed to create designation");
}

/// Creates a new document category via the roles-management API, using the caller's JWT
/// (which must hold `documents.categories.manage`). Only PUBLIC/RESIDENT_SPECIFIC are
/// seeded, so tests must create any other category before uploading documents into it.
pub fn quick_create_document_category(env: &TestEnv, caller_jwt: &str, code: &str, label: &str) {
    let url = env.server_url();
    let _ = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-categories/create",
        url
    ))
    .set("Authorization", &format!("Bearer {}", caller_jwt))
    .send_json(serde_json::json!({ "code": code, "label": label }))
    .expect("Failed to create document category");
}

/// Creates a subcategory under `category` via the roles-management API, using the caller's
/// JWT (which must hold `documents.categories.manage`).
pub fn quick_create_document_subcategory(
    env: &TestEnv,
    caller_jwt: &str,
    category: &str,
    code: &str,
    label: &str,
) {
    let url = env.server_url();
    let _ = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-subcategories/create",
        url
    ))
    .set("Authorization", &format!("Bearer {}", caller_jwt))
    .send_json(serde_json::json!({ "category": category, "code": code, "label": label }))
    .expect("Failed to create document subcategory");
}

/// Grants or revokes a permission for a designation via the roles-management API, using
/// the caller's JWT (which must hold `designations.manage`).
pub fn quick_toggle_designation_permission(
    env: &TestEnv,
    caller_jwt: &str,
    designation_code: &str,
    permission_code: &str,
    enabled: bool,
) {
    let url = env.server_url();
    let _ = ureq::post(&format!(
        "{}/api/v1/admin/roles/designations/permissions/toggle",
        url
    ))
    .set("Authorization", &format!("Bearer {}", caller_jwt))
    .send_json(serde_json::json!({
        "designation_code": designation_code,
        "permission_code": permission_code,
        "enabled": enabled
    }))
    .expect("Failed to toggle designation permission");
}

/// Assigns a designation to a user via the roles-management API, using the caller's JWT
/// (which must hold `users.roles.manage`).
pub fn quick_assign_designation(
    env: &TestEnv,
    caller_jwt: &str,
    user_id: i32,
    designation_code: &str,
) {
    let url = env.server_url();
    let _ = ureq::post(&format!("{}/api/v1/admin/roles/designations/assign", url))
        .set("Authorization", &format!("Bearer {}", caller_jwt))
        .send_json(serde_json::json!({
            "user_id": user_id,
            "designation_code": designation_code
        }))
        .expect("Failed to assign designation");
}

/// Revokes a designation from a user via the roles-management API, using the caller's JWT
/// (which must hold `users.roles.manage`).
pub fn quick_revoke_designation(
    env: &TestEnv,
    caller_jwt: &str,
    user_id: i32,
    designation_code: &str,
) {
    let url = env.server_url();
    let _ = ureq::post(&format!("{}/api/v1/admin/roles/designations/revoke", url))
        .set("Authorization", &format!("Bearer {}", caller_jwt))
        .send_json(serde_json::json!({
            "user_id": user_id,
            "designation_code": designation_code
        }))
        .expect("Failed to revoke designation");
}

/// Grants or revokes a designation's access to a document category via the roles-management
/// API, using the caller's JWT (which must hold `designations.manage`).
pub fn quick_set_category_designation(
    env: &TestEnv,
    caller_jwt: &str,
    category: &str,
    designation_code: &str,
    can_read: bool,
    can_manage: bool,
) {
    let url = env.server_url();
    let _ = ureq::post(&format!(
        "{}/api/v1/admin/roles/document-category-designations/toggle",
        url
    ))
    .set("Authorization", &format!("Bearer {}", caller_jwt))
    .send_json(serde_json::json!({
        "category": category,
        "designation_code": designation_code,
        "can_read": can_read,
        "can_manage": can_manage
    }))
    .expect("Failed to set category designation");
}

/// Programmatically creates a document via multipart upload using Admin JWT, returning the `document_id`.
pub fn quick_create_document(
    env: &TestEnv,
    admin_jwt: &str,
    title: &str,
    category: &str,
    flat: Option<(&str, &str)>,
) -> String {
    let url = env.server_url();
    let mock_bytes = b"Quick framework helper seeded file document bytes.";

    let mut payload = serde_json::json!({
        "title": title,
        "filename": "seeded.txt",
        "description": "Programmatic helper seeded document",
        "category": category,
        "metadata": {
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

    let url_resp = ureq::post(&format!("{}/api/v1/admin/documents/upload-url", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(payload)
        .expect("Failed to create upload url");

    let val: serde_json::Value = url_resp.into_json().unwrap();
    let document_id = val["document_id"]
        .as_str()
        .expect("document_id missing")
        .to_string();
    let presigned_url = val["upload_url"]
        .as_str()
        .expect("upload_url missing")
        .to_string();

    let put_resp = ureq::put(&presigned_url)
        .set("Content-Type", "text/plain")
        .send_bytes(mock_bytes)
        .expect("Failed to put bytes to presigned URL");
    let _ = put_resp.into_string();

    let confirm_resp = ureq::post(&format!("{}/api/v1/admin/documents/confirm-upload", url))
        .set("Authorization", &format!("Bearer {}", admin_jwt))
        .send_json(serde_json::json!({
            "document_id": document_id
        }));

    if let Err(ureq::Error::Status(code, response)) = confirm_resp {
        panic!(
            "Failed to confirm upload: {} - {}",
            code,
            response.into_string().unwrap()
        );
    }

    document_id
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
