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
}

impl AppConfig {
    pub fn load_config() -> Self {
        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "local".to_string());
        let file_name = format!("config_{}.toml", env);

        println!("Loading configuration from {}", file_name);

        let config_str = std::fs::read_to_string(&file_name)
            .unwrap_or_else(|e| panic!("Failed to read configuration file {}: {}", file_name, e));

        let config: Self = toml::from_str(&config_str)
            .unwrap_or_else(|e| panic!("Failed to parse configuration file {}: {}", file_name, e));

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
