use base::config::{PostgresConfig, S3Config};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct TelemetryConfig {
    pub otlp_endpoint: Option<String>,
    pub otlp_username: Option<String>,
    pub otlp_password: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub ip_per_min: u32,
    pub user_per_min: u32,
    pub admin_per_min: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Config for the SignYu e-signature provider. `api_key` and `webhook_secret` are
/// secrets — always sourced from env vars, never committed to a config_*.toml.
/// `webhook_secret` is the HMAC-SHA256 signing secret SignYu's dashboard shows for a
/// configured webhook endpoint — used to verify the `X-SignSetu-Signature` header on
/// every inbound `POST /webhooks/signyu` call; a webhook received with no secret
/// configured is rejected outright (unverifiable input is untrusted input).
/// `base_url` defaults to the real SignYu API but is overridable so tests (and any
/// future staging setup) can point the client at a local mock server instead.
#[derive(Debug, Deserialize, Clone)]
pub struct SignYuConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub webhook_secret: Option<String>,
    #[serde(default = "default_signyu_base_url")]
    pub base_url: String,
}

fn default_signyu_base_url() -> String {
    "https://signyu.com/api/v1".to_string()
}

impl Default for SignYuConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            webhook_secret: None,
            base_url: default_signyu_base_url(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub super_admin_email: String,
    pub postgres: PostgresConfig,
    pub s3: S3Config,
    pub jwt_secret: String,
    #[serde(with = "humantime_serde")]
    pub jwt_expiry: std::time::Duration,
    #[serde(with = "humantime_serde")]
    pub forgot_password_expiry: std::time::Duration,
    pub telemetry: TelemetryConfig,
    pub rate_limit: RateLimitConfig,
    #[serde(default)]
    pub signyu: SignYuConfig,
}

impl AppConfig {
    pub fn load_config() -> Self {
        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "local".to_string());
        let file_name = format!("config_{}.toml", env);

        let config_str = std::fs::read_to_string(&file_name)
            .unwrap_or_else(|e| panic!("Failed to read configuration file {}: {}", file_name, e));

        let mut config: Self = toml::from_str(&config_str)
            .unwrap_or_else(|e| panic!("Failed to parse configuration file {}: {}", file_name, e));

        // Override settings with environment variables if present
        if let Ok(jwt_secret) = std::env::var("JWT_SECRET") {
            config.jwt_secret = jwt_secret;
        }
        if let Ok(db_url) = std::env::var("DATABASE_URL") {
            config.postgres.database_url = Some(db_url);
        }
        if let Ok(db_host) = std::env::var("DATABASE_HOST") {
            config.postgres.host = db_host;
        }
        if let Ok(db_port) = std::env::var("DATABASE_PORT") {
            if let Ok(port) = db_port.parse::<u16>() {
                config.postgres.port = port;
            }
        }
        if let Ok(db_username) = std::env::var("DATABASE_USERNAME") {
            config.postgres.username = db_username;
        }
        if let Ok(db_password) = std::env::var("DATABASE_PASSWORD") {
            config.postgres.password = Some(db_password);
        }
        if let Ok(db_name) = std::env::var("DATABASE_NAME") {
            config.postgres.database = db_name;
        }
        if let Ok(s3_region) = std::env::var("S3_REGION") {
            config.s3.region = s3_region;
        }
        if let Ok(s3_bucket) = std::env::var("S3_BUCKET_NAME") {
            config.s3.bucket_name = s3_bucket;
        }
        if let Ok(super_admin_email) = std::env::var("SUPER_ADMIN_EMAIL") {
            config.super_admin_email = super_admin_email;
        }
        // A set-but-empty env var (e.g. `SIGNYU_API_KEY=` in a compose file with no
        // value supplied) must NOT be treated as "configured" — `api_key.is_some()` is
        // literally the on/off switch for the whole SignYu provider, so an empty
        // string here would silently enable it with a blank key.
        if let Ok(signyu_api_key) = std::env::var("SIGNYU_API_KEY") {
            if !signyu_api_key.is_empty() {
                config.signyu.api_key = Some(signyu_api_key);
            }
        }
        if let Ok(signyu_webhook_secret) = std::env::var("SIGNYU_WEBHOOK_SECRET") {
            if !signyu_webhook_secret.is_empty() {
                config.signyu.webhook_secret = Some(signyu_webhook_secret);
            }
        }
        if let Ok(signyu_base_url) = std::env::var("SIGNYU_BASE_URL") {
            if !signyu_base_url.is_empty() {
                config.signyu.base_url = signyu_base_url;
            }
        }

        config
    }

    /// Helper to get database configuration.
    pub fn get_postgres_config(&self) -> PostgresConfig {
        self.postgres.clone()
    }

    /// Helper to get S3 storage configuration.
    #[allow(dead_code)]
    pub fn get_s3_config(&self) -> S3Config {
        self.s3.clone()
    }
}
