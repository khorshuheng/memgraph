use std::collections::HashSet;
use std::sync::Arc;

use crate::{model::AccessAction, repository::graph::GraphRepository};

#[derive(Clone)]
pub struct AuditService {
    repository: Arc<dyn GraphRepository>,
}

impl AuditService {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }

    pub async fn record(&self, session_id: &str, node_ids: &[i64], action: AccessAction) {
        let mut seen = HashSet::new();
        let unique: Vec<i64> = node_ids
            .iter()
            .copied()
            .filter(|node_id| seen.insert(*node_id))
            .collect();
        if unique.is_empty() {
            return;
        }
        if let Err(error) = self
            .repository
            .record_node_accesses(session_id, &unique, action)
            .await
        {
            tracing::warn!(
                %error,
                session_id = %session_id,
                node_count = unique.len(),
                "failed to record node access"
            );
        }
    }

    pub async fn prune(&self, retention_days: u64) {
        match self.repository.prune_node_access(retention_days).await {
            Ok(removed) if removed > 0 => {
                tracing::info!(removed, retention_days, "pruned node access history")
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "failed to prune node access history"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::{create_sqlite_pool, migrate},
        model::{Node, NodeKind},
        repository::graph::SqliteGraphRepository,
    };
    use sqlx::SqlitePool;

    async fn service() -> (AuditService, Arc<dyn GraphRepository>, SqlitePool) {
        let config = crate::config::DatabaseConfig {
            path: ":memory:".to_string(),
            max_connections: 1,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");
        let repository: Arc<dyn GraphRepository> =
            Arc::new(SqliteGraphRepository::new(pool.clone()));
        (AuditService::new(repository.clone()), repository, pool)
    }

    async fn seed_node(repository: &Arc<dyn GraphRepository>) -> i64 {
        let kind = repository
            .create_node_kind(&NodeKind {
                id: 0,
                name: "widget".to_string(),
                description: String::new(),
                updated_at: String::new(),
            })
            .await
            .expect("create node kind");
        repository
            .upsert_node(&Node {
                id: 0,
                kind_id: kind.id,
                name: "a.widget".to_string(),
                description: String::new(),
                content: String::new(),
                updated_at: String::new(),
            })
            .await
            .expect("create node")
            .id
    }

    #[tokio::test]
    async fn records_session_node_action_and_time() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;

        audit
            .record("session-1", &[node_id], AccessAction::Write)
            .await;

        let (session_id, stored_node_id, node_name, node_kind, action, accessed_at): (
            String,
            i64,
            String,
            String,
            String,
            String,
        ) = sqlx::query_as(
            "SELECT session_id, node_id, node_name, node_kind, action, accessed_at FROM node_access",
        )
        .fetch_one(&pool)
        .await
        .expect("audit row");
        assert_eq!(session_id, "session-1");
        assert_eq!(stored_node_id, node_id);
        assert_eq!(node_name, "a.widget");
        assert_eq!(node_kind, "widget");
        assert_eq!(action, "write");
        assert!(!accessed_at.is_empty());
    }

    #[tokio::test]
    async fn skips_duplicates_and_unknown_nodes() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;

        audit
            .record("session-1", &[node_id, node_id, 9999], AccessAction::Read)
            .await;

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM node_access")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn keeps_snapshot_after_node_is_deleted() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;
        audit
            .record("session-1", &[node_id], AccessAction::Read)
            .await;
        repository.delete_node(node_id).await.expect("delete node");

        let name: String = sqlx::query_scalar("SELECT node_name FROM node_access")
            .fetch_one(&pool)
            .await
            .expect("audit row survives node deletion");
        assert_eq!(name, "a.widget");
    }

    #[tokio::test]
    async fn records_many_nodes_in_one_batch() {
        let (audit, repository, pool) = service().await;
        let kind = repository
            .create_node_kind(&NodeKind {
                id: 0,
                name: "widget".to_string(),
                description: String::new(),
                updated_at: String::new(),
            })
            .await
            .expect("create node kind");
        let mut node_ids = Vec::new();
        for index in 0..50 {
            node_ids.push(
                repository
                    .upsert_node(&Node {
                        id: 0,
                        kind_id: kind.id,
                        name: format!("n{index}"),
                        description: String::new(),
                        content: String::new(),
                        updated_at: String::new(),
                    })
                    .await
                    .expect("create node")
                    .id,
            );
        }

        audit
            .record("session-1", &node_ids, AccessAction::Read)
            .await;

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM node_access")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 50);
    }

    #[tokio::test]
    async fn prune_removes_only_rows_past_retention() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;
        audit.record("old", &[node_id], AccessAction::Read).await;
        sqlx::query(
            "UPDATE node_access SET accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-100 days')",
        )
        .execute(&pool)
        .await
        .expect("age the row");
        audit.record("new", &[node_id], AccessAction::Read).await;

        audit.prune(30).await;

        let sessions: Vec<String> = sqlx::query_scalar("SELECT session_id FROM node_access")
            .fetch_all(&pool)
            .await
            .expect("remaining rows");
        assert_eq!(sessions, vec!["new".to_string()]);
    }
}
