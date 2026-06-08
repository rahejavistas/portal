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
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub admin_email: String,
    pub postgres: PostgresConfig,
    pub s3: S3Config,
    pub jwt_secret: String,
    #[serde(with = "humantime_serde")]
    pub jwt_expiry: std::time::Duration,
    #[serde(with = "humantime_serde")]
    pub forgot_password_expiry: std::time::Duration,
    pub telemetry: TelemetryConfig,
    pub rate_limit: RateLimitConfig,
    pub log_reset_link: Option<bool>,
}

impl AppConfig {
    pub fn load_config() -> Self {
        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "local".to_string());
        let file_name = format!("config_{}.toml", env);

        println!("Loading configuration from {}", file_name);

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
        if let Ok(admin_email) = std::env::var("ADMIN_EMAIL") {
            config.admin_email = admin_email;
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
