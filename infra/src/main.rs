use infra::db;
use std::env;

use serde::Deserialize;

use base::config::PostgresConfig;

#[derive(Deserialize)]
struct Config {
    postgres: PostgresConfig,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_env = env::var("APP_ENV").unwrap_or_else(|_| "local".to_string());
    let file_name = format!("config_{}.toml", app_env);

    let config_str = std::fs::read_to_string(&file_name)
        .unwrap_or_else(|_| panic!("Failed to read configuration file: {}", file_name));
    let config: Config = toml::from_str(&config_str).expect("Failed to parse configuration");

    // Parse arguments manually (if --host, etc. are passed, use them, otherwise use config)
    let mut args = env::args().skip(1);

    let mut host = config.postgres.host;
    let mut port: u16 = config.postgres.port;
    let mut username = config.postgres.username;
    let mut password = config.postgres.password;
    let mut database = config.postgres.database;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => {
                if let Some(val) = args.next() {
                    host = val;
                }
            }
            "--port" => {
                if let Some(val) = args.next() {
                    port = val.parse().unwrap_or(port);
                }
            }
            "--username" => {
                if let Some(val) = args.next() {
                    username = val;
                }
            }
            "--password" => {
                if let Some(val) = args.next() {
                    password = Some(val);
                }
            }
            "--database" => {
                if let Some(val) = args.next() {
                    database = val;
                }
            }
            _ => {}
        }
    }

    let password_part = password
        .as_deref()
        .map(|p| format!(":{}", p))
        .unwrap_or_default();
    let db_url = format!(
        "postgres://{}{password_part}@{}:{}/{}",
        username, host, port, database
    );

    println!(
        "Connecting to database {} at {}:{} as user {}",
        database, host, port, username
    );

    let pool = db::create_pool(&db_url).await?;

    println!("Running migrations...");
    db::run_migrations(&pool).await?;

    println!("Migrations complete.");
    Ok(())
}
