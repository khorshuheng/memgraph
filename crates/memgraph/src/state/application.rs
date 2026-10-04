use std::sync::Arc;

use crate::{
    config::AppConfig, database, repository::graph::SqliteGraphRepository,
    service::audit::AuditService, service::graph::GraphService, service::health::HealthService,
};

#[derive(Clone)]
pub struct AppState {
    pub graph_service: GraphService,
    pub health_service: HealthService,
    pub audit_service: AuditService,
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
        let health_service = HealthService::new(graph_repository.clone());
        let audit_service = AuditService::new(graph_repository);
        let state = Self {
            graph_service,
            health_service,
            audit_service,
        };
        state.spawn_health_reporter(&app_config.health);
        state.spawn_audit_pruner(&app_config.audit);
        state
    }

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

    fn spawn_audit_pruner(&self, audit_config: &crate::config::AuditConfig) {
        if audit_config.retention_days == 0 {
            tracing::info!("node access retention disabled");
            return;
        }

        let audit_service = self.audit_service.clone();
        let retention_days = audit_config.retention_days;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                ticker.tick().await;
                audit_service.prune(retention_days).await;
            }
        });
    }
}
