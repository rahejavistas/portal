use crate::config::PostgresConfig;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use std::time::Duration;

pub async fn create_pool(config: &PostgresConfig) -> Result<Pool<Postgres>, sqlx::Error> {
    let password_part = config
        .password
        .as_deref()
        .map(|p| format!(":{}", p))
        .unwrap_or_default();
    let db_url = format!(
        "postgres://{}{password_part}@{}:{}/{}",
        config.username, config.host, config.port, config.database
    );

    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&db_url)
        .await
}

/// Begins a new database transaction from the connection pool.
/// The returned transaction will automatically rollback on drop if `.commit()` is not called,
/// ensuring atomicity: either all writes succeed, or none persist.
pub async fn begin_transaction(
    pool: &Pool<Postgres>,
) -> Result<sqlx::Transaction<'_, Postgres>, sqlx::Error> {
    pool.begin().await
}
