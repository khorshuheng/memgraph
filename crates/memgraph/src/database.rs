use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

use crate::config::DatabaseConfig;

pub async fn create_sqlite_pool(
    database_config: &DatabaseConfig,
) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(&database_config.path)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10));
    SqlitePoolOptions::new()
        .max_connections(database_config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DatabaseConfig;

    #[tokio::test]
    async fn file_pools_use_wal_and_a_busy_timeout() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "memgraph-wal-{}-{unique}.sqlite",
            std::process::id()
        ));
        let config = DatabaseConfig {
            path: path.to_string_lossy().into_owned(),
            max_connections: 1,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");

        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("journal mode");
        let timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&pool)
            .await
            .expect("busy timeout");
        assert_eq!(mode, "wal");
        assert_eq!(timeout, 10_000);

        drop(pool);
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(path.with_file_name(format!("{file}-wal"))).ok();
        std::fs::remove_file(path.with_file_name(format!("{file}-shm"))).ok();
    }
}
