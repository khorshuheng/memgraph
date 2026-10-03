use sqlx::SqlitePool;

use crate::config::AppConfig;

use super::database::create_sqlite_pool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

impl AppState {
    pub async fn new(app_config: &AppConfig) -> Self {
        let pool = create_sqlite_pool(&app_config.database)
            .await
            .expect("failed to create database connection pool");
        Self { pool }
    }
}
