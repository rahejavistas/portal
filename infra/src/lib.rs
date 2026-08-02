pub mod db {
    use sqlx::{postgres::PgConnectOptions, postgres::PgPoolOptions, Pool, Postgres};
    use std::str::FromStr;
    use std::time::Duration;

    /// Runs all pending migrations from the `migrations` directory, embedded at compile time.
    pub async fn run_migrations(pool: &Pool<Postgres>) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(pool).await
    }

    /// Helper to create a connection pool
    pub async fn create_pool(database_url: &str) -> Result<Pool<Postgres>, sqlx::Error> {
        let connect_options = PgConnectOptions::from_str(database_url)?.statement_cache_capacity(0);

        PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(3))
            .connect_with(connect_options)
            .await
    }
}
