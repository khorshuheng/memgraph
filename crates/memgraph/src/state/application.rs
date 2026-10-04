use std::sync::Arc;

use crate::{
    config::AppConfig, database, repository::graph::SqliteGraphRepository,
    service::graph::GraphService, service::health::HealthService,
};

#[derive(Clone)]
pub struct AppState {
    pub graph_service: GraphService,
    pub health_service: HealthService,
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
        let graph_service = GraphService::new(graph_repository.clone(), app_config.search.clone());
        let health_service = HealthService::new(graph_repository);
        let state = Self {
            graph_service,
            health_service,
        };
        state.spawn_health_reporter(&app_config.health);
        state
    }

    /// Logs the graph shape report at startup and on every tick. The report is
    /// read-only, so this task holds no locks worth releasing and is simply
    /// dropped with the runtime at shutdown.
    fn spawn_health_reporter(&self, health_config: &crate::config::HealthConfig) {
        if !health_config.enabled {
            tracing::info!("periodic graph health reporting disabled");
            return;
        }

        let health_service = self.health_service.clone();
        let interval = health_config.interval();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                // The first tick completes immediately, so this doubles as the
                // startup report.
                ticker.tick().await;
                match health_service.report().await {
                    Ok(report) => {
                        tracing::info!(health = %report.summary_line(), "graph health");
                        for duplicate in report.duplicates.iter().take(5) {
                            tracing::warn!(
                                kind = %duplicate.kind,
                                name = %duplicate.name,
                                node_ids = ?duplicate.node_ids,
                                "duplicate nodes share a kind and name"
                            );
                        }
                    }
                    Err(error) => tracing::warn!(%error, "graph health report failed"),
                }
            }
        });
    }
}
