use std::sync::Arc;

use crate::{
    config::AppConfig, database, repository::graph::SqliteGraphRepository,
    service::graph::GraphService,
};

#[derive(Clone)]
pub struct AppState {
    pub graph_service: GraphService,
}

impl AppState {
    pub async fn new(app_config: &AppConfig) -> Self {
        let pool = database::create_sqlite_pool(&app_config.database)
            .await
            .expect("failed to create database connection pool");
        database::migrate(&pool)
            .await
            .expect("failed to migrate database");
        let graph_repository = Arc::new(SqliteGraphRepository::new(pool));
        let graph_service = GraphService::new(graph_repository);
        Self { graph_service }
    }
}
