use crate::config::PostgresConfig;
use sqlx::{postgres::PgConnectOptions, postgres::PgPoolOptions, Pool, Postgres};
use std::str::FromStr;
use std::time::Duration;

pub async fn create_pool(config: &PostgresConfig) -> Result<Pool<Postgres>, sqlx::Error> {
    let db_url = if let Some(ref url) = config.database_url {
        url.clone()
    } else {
        let password_part = config
            .password
            .as_deref()
            .map(|p| format!(":{}", p))
            .unwrap_or_default();
        format!(
            "postgres://{}{password_part}@{}:{}/{}",
            config.username, config.host, config.port, config.database
        )
    };

    let connect_options = PgConnectOptions::from_str(&db_url)?.statement_cache_capacity(0);

    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .connect_with(connect_options)
        .await
}

/// Rolls back on drop if `.commit()` isn't called.
pub async fn begin_transaction(
    pool: &Pool<Postgres>,
) -> Result<sqlx::Transaction<'_, Postgres>, sqlx::Error> {
    pool.begin().await
}
