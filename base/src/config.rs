use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct PostgresConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_username")]
    pub username: String,
    pub password: Option<String>,
    #[serde(default = "default_database")]
    pub database: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    // TODO: remove this
    pub database_url: Option<String>,
}

fn default_host() -> String {
    "localhost".to_string()
}
fn default_port() -> u16 {
    5432
}
fn default_username() -> String {
    "postgres".to_string()
}
fn default_database() -> String {
    "postgres".to_string()
}
fn default_max_connections() -> u32 {
    5
}

#[derive(Debug, Deserialize, Clone)]
pub struct S3Config {
    pub region: String,
    pub bucket_name: String,
    pub endpoint_url: Option<String>,
    /// Public-facing endpoint for presigned URLs (browser-accessible).
    /// Falls back to `endpoint_url` if not set.
    pub public_endpoint_url: Option<String>,
    #[serde(default, with = "humantime_serde")]
    pub presigned_expiry: Option<Duration>,
}
