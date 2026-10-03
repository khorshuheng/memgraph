use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::config::DatabaseConfig;

pub async fn create_sqlite_pool(
    database_config: &DatabaseConfig,
) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(&database_config.path)?.create_if_missing(true);
    SqlitePoolOptions::new()
        .max_connections(database_config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
}
