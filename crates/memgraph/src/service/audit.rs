use std::collections::HashSet;
use std::sync::Arc;

use crate::{
    model::AccessAction,
    repository::graph::{GraphRepository, NodeAccessRecord},
    service::error::ServiceError,
};

#[derive(Clone)]
pub struct AuditService {
    repository: Arc<dyn GraphRepository>,
}

impl AuditService {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }

    pub async fn record(
        &self,
        session_id: &str,
        node_ids: &[i64],
        action: AccessAction,
        query_term: Option<&str>,
    ) -> Vec<NodeAccessRecord> {
        let mut seen = HashSet::new();
        let unique: Vec<i64> = node_ids
            .iter()
            .copied()
            .filter(|node_id| seen.insert(*node_id))
            .collect();
        if unique.is_empty() {
            return Vec::new();
        }
        match self
            .repository
            .record_node_accesses(session_id, &unique, action, query_term)
            .await
        {
            Ok(recorded) => recorded,
            Err(error) => {
                tracing::warn!(
                    %error,
                    session_id = %session_id,
                    node_count = unique.len(),
                    "failed to record node access"
                );
                Vec::new()
            }
        }
    }

    pub async fn accessed_node_ids(&self, session_id: &str) -> HashSet<i64> {
        match self.repository.accessed_node_ids(session_id).await {
            Ok(ids) => ids,
            Err(error) => {
                tracing::warn!(
                    %error,
                    session_id = %session_id,
                    "failed to load accessed nodes"
                );
                HashSet::new()
            }
        }
    }

    pub async fn mark_relevance(
        &self,
        session_id: &str,
        access_id: i64,
        relevant: bool,
    ) -> Result<(), ServiceError> {
        let updated = self
            .repository
            .set_node_access_relevance(session_id, access_id, relevant)
            .await?;
        if updated {
            Ok(())
        } else {
            Err(ServiceError::EntityNotFound)
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

    async fn seed_kind(repository: &Arc<dyn GraphRepository>) -> i64 {
        repository
            .create_node_kind(&NodeKind {
                id: 0,
                name: "widget".to_string(),
                description: String::new(),
                updated_at: String::new(),
            })
            .await
            .expect("create node kind")
            .id
    }

    async fn seed_named_node(
        repository: &Arc<dyn GraphRepository>,
        kind_id: i64,
        name: &str,
    ) -> i64 {
        repository
            .upsert_node(&Node {
                id: 0,
                kind_id,
                name: name.to_string(),
                description: String::new(),
                content: String::new(),
                updated_at: String::new(),
            })
            .await
            .expect("create node")
            .id
    }

    async fn seed_node(repository: &Arc<dyn GraphRepository>) -> i64 {
        let kind_id = seed_kind(repository).await;
        seed_named_node(repository, kind_id, "a.widget").await
    }

    #[tokio::test]
    async fn records_session_node_action_and_time() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;

        audit
            .record("session-1", &[node_id], AccessAction::Write, None)
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
    async fn lists_node_ids_recorded_for_a_session() {
        let (audit, repository, _) = service().await;
        let kind_id = seed_kind(&repository).await;
        let first = seed_named_node(&repository, kind_id, "a.widget").await;
        let second = seed_named_node(&repository, kind_id, "b.widget").await;

        audit
            .record("session-1", &[first, second], AccessAction::Read, None)
            .await;
        audit
            .record("session-2", &[first], AccessAction::Read, None)
            .await;

        assert_eq!(
            audit.accessed_node_ids("session-1").await,
            HashSet::from([first, second])
        );
        assert_eq!(
            audit.accessed_node_ids("session-2").await,
            HashSet::from([first])
        );
        assert!(audit.accessed_node_ids("unknown").await.is_empty());
    }

    #[tokio::test]
    async fn skips_duplicates_and_unknown_nodes() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;

        audit
            .record(
                "session-1",
                &[node_id, node_id, 9999],
                AccessAction::Read,
                None,
            )
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
            .record("session-1", &[node_id], AccessAction::Read, None)
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
            .record("session-1", &node_ids, AccessAction::Read, None)
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
        audit
            .record("old", &[node_id], AccessAction::Read, None)
            .await;
        sqlx::query(
            "UPDATE node_access SET accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-100 days')",
        )
        .execute(&pool)
        .await
        .expect("age the row");
        audit
            .record("new", &[node_id], AccessAction::Read, None)
            .await;

        audit.prune(30).await;

        let sessions: Vec<String> = sqlx::query_scalar("SELECT session_id FROM node_access")
            .fetch_all(&pool)
            .await
            .expect("remaining rows");
        assert_eq!(sessions, vec!["new".to_string()]);
    }

    #[tokio::test]
    async fn records_query_term_and_defaults_relevance_to_null() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;

        audit
            .record(
                "session-1",
                &[node_id],
                AccessAction::Read,
                Some("rotated token"),
            )
            .await;

        let (query_term, relevant): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT query_term, relevant FROM node_access")
                .fetch_one(&pool)
                .await
                .expect("audit row");
        assert_eq!(query_term.as_deref(), Some("rotated token"));
        assert_eq!(relevant, None);
    }

    #[tokio::test]
    async fn returns_node_and_access_ids_in_their_own_fields() {
        let (audit, repository, _pool) = service().await;
        let kind_id = seed_kind(&repository).await;
        let _first = seed_named_node(&repository, kind_id, "a").await;
        let second = seed_named_node(&repository, kind_id, "b").await;

        let recorded = audit
            .record("session-1", &[second], AccessAction::Read, None)
            .await;

        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].node_id, second);
        assert_eq!(recorded[0].access_id, 1);
        assert_ne!(recorded[0].node_id, recorded[0].access_id);
    }

    #[tokio::test]
    async fn marks_relevance_only_for_the_owning_session() {
        let (audit, repository, pool) = service().await;
        let node_id = seed_node(&repository).await;
        let recorded = audit
            .record("session-1", &[node_id], AccessAction::Read, None)
            .await;
        let access_id = recorded[0].access_id;

        assert_eq!(
            audit.mark_relevance("session-2", access_id, true).await,
            Err(ServiceError::EntityNotFound)
        );
        assert_eq!(
            audit.mark_relevance("session-1", 9999, true).await,
            Err(ServiceError::EntityNotFound)
        );
        assert_eq!(
            audit.mark_relevance("session-1", access_id, true).await,
            Ok(())
        );

        let relevant: Option<i64> = sqlx::query_scalar("SELECT relevant FROM node_access")
            .fetch_one(&pool)
            .await
            .expect("audit row");
        assert_eq!(relevant, Some(1));
    }
}
