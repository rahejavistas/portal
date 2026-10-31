use base::config::{PostgresConfig, S3Config};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct TelemetryConfig {
    pub otlp_endpoint: Option<String>,
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

/// SignYu e-signature provider config. `api_key`/`webhook_secret` are secrets, always
/// from env vars. `base_url` defaults to the real API but is overridable for tests.
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
    #[serde(default)]
    pub accounting_enabled: bool,
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
        match std::env::var("JWT_SECRET") {
            Ok(jwt_secret) => config.jwt_secret = jwt_secret,
            Err(_) if env == "prod" => panic!(
                "JWT_SECRET must be set via environment variable in production — \
                 refusing to fall back to the placeholder secret committed in config_prod.toml"
            ),
            Err(_) => {}
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
        // A set-but-empty var must not count as configured — it's the on/off switch.
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
        if let Ok(accounting_enabled) = std::env::var("ACCOUNTING_ENABLED") {
            config.accounting_enabled = accounting_enabled == "true";
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
