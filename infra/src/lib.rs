pub mod db {
    use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
    use std::time::Duration;

    /// Runs all pending migrations in the `migrations` directory.
    /// This uses the `sqlx::migrate!` macro to embed the SQL files in the binary
    /// and ensures a `_sqlx_migrations` table is automatically maintained in the DB.
    pub async fn run_migrations(pool: &Pool<Postgres>) -> Result<(), sqlx::migrate::MigrateError> {
        // This macro looks for a `migrations` directory in the root of the crate.
        // It will embed the migration files into the final executable.
        sqlx::migrate!("./migrations").run(pool).await
    }

    /// Helper to create a connection pool
    pub async fn create_pool(database_url: &str) -> Result<Pool<Postgres>, sqlx::Error> {
        PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(3))
            .connect(database_url)
            .await
    }
}
