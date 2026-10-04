use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    model::{DuplicateGroup, GraphHealth},
    repository::graph::GraphRepository,
};

use super::error::ServiceError;

/// How many isolated nodes are returned as detail rows. The report always
/// carries the true total in `isolated_count`; this only bounds the payload.
pub const ISOLATED_NODE_SAMPLE: i64 = 50;

/// Read-only introspection of the stored graph.
///
/// Every measurement here is derived from data that is already persisted, so
/// the report needs no schema change and no write path. It exists to make dead
/// vocabulary, duplicate nodes and unreachable nodes visible, so consolidation
/// can later be driven by evidence instead of by guesswork.
#[derive(Clone)]
pub struct HealthService {
    repository: Arc<dyn GraphRepository>,
}

impl HealthService {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }

    pub async fn report(&self) -> Result<GraphHealth, ServiceError> {
        self.report_with_sample(ISOLATED_NODE_SAMPLE).await
    }

    pub async fn report_with_sample(&self, sample: i64) -> Result<GraphHealth, ServiceError> {
        let kinds = self.repository.kind_usage().await?;
        let edge_types = self.repository.edge_type_usage().await?;
        let duplicate_rows = self.repository.duplicate_node_rows().await?;
        let isolated_count = self.repository.isolated_node_count().await?;
        let isolated = self.repository.isolated_nodes(sample).await?;

        // Totals come from the same rows the report displays, so the summary and
        // the detail can never disagree.
        let node_count = kinds.iter().map(|usage| usage.node_count).sum();
        let edge_count = edge_types.iter().map(|usage| usage.edge_count).sum();

        Ok(GraphHealth {
            node_count,
            edge_count,
            kinds,
            edge_types,
            duplicates: group_duplicates(duplicate_rows),
            isolated_count,
            isolated,
        })
    }
}

/// Folds `(kind_id, kind, name, node_id)` rows into one group per name, keeping
/// ids ascending and group order stable, so repeated runs report identically.
fn group_duplicates(rows: Vec<(i64, String, String, i64)>) -> Vec<DuplicateGroup> {
    let mut groups: Vec<DuplicateGroup> = Vec::new();
    let mut index: HashMap<(i64, String), usize> = HashMap::new();

    for (kind_id, kind, name, node_id) in rows {
        let key = (kind_id, name.clone());
        match index.get(&key) {
            Some(position) => groups[*position].node_ids.push(node_id),
            None => {
                index.insert(key, groups.len());
                groups.push(DuplicateGroup {
                    kind_id,
                    kind,
                    name,
                    node_ids: vec![node_id],
                });
            }
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        database::{create_sqlite_pool, migrate},
        model::{Edge, Node},
        repository::graph::SqliteGraphRepository,
    };

    use super::*;

    async fn service() -> (HealthService, Arc<dyn GraphRepository>) {
        let config = crate::config::DatabaseConfig {
            path: ":memory:".to_string(),
            max_connections: 1,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");
        let repository: Arc<dyn GraphRepository> = Arc::new(SqliteGraphRepository::new(pool));
        (HealthService::new(repository.clone()), repository)
    }

    async fn node(repository: &Arc<dyn GraphRepository>, kind_id: i64, name: &str) -> i64 {
        repository
            .upsert_node(&Node {
                id: 0,
                kind_id,
                name: name.to_string(),
                description: format!("{name} description"),
                content: String::new(),
            })
            .await
            .expect("insert node")
            .id
    }

    /// The seeded vocabulary means a fresh database already has dead kinds and
    /// underused edge types, which is exactly what the report should surface.
    #[tokio::test]
    async fn reports_seeded_vocabulary_as_unused() {
        let (service, _) = service().await;
        let report = service.report().await.expect("report");

        assert_eq!(report.node_count, 0);
        assert_eq!(report.edge_count, 0);
        assert_eq!(report.dead_kinds().count(), report.kinds.len());
        assert_eq!(
            report.underused_edge_types().count(),
            report.edge_types.len()
        );
        assert!(report.duplicates.is_empty());
        assert_eq!(report.isolated_count, 0);
    }

    #[tokio::test]
    async fn counts_nodes_per_kind_and_clears_dead_kinds() {
        let (service, repository) = service().await;
        let file_kind = repository
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "file")
            .expect("seeded file kind");

        node(&repository, file_kind.id, "a.rs").await;
        node(&repository, file_kind.id, "b.rs").await;

        let report = service.report().await.expect("report");
        let file_usage = report
            .kinds
            .iter()
            .find(|usage| usage.kind_id == file_kind.id)
            .expect("file usage");
        assert_eq!(file_usage.node_count, 2);
        assert_eq!(report.node_count, 2);
        assert!(
            !report
                .dead_kinds()
                .any(|usage| usage.kind_id == file_kind.id)
        );
    }

    #[tokio::test]
    async fn groups_duplicate_nodes_by_kind_and_name() {
        let (service, repository) = service().await;
        let file_kind = repository
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "file")
            .expect("seeded file kind");

        // Nothing enforces uniqueness on (kind_id, name): this is the duplicate
        // proliferation the report exists to expose.
        let first = node(&repository, file_kind.id, "src/main.rs").await;
        let second = node(&repository, file_kind.id, "src/main.rs").await;
        node(&repository, file_kind.id, "src/lib.rs").await;

        let report = service.report().await.expect("report");
        assert_eq!(report.duplicates.len(), 1);
        let group = &report.duplicates[0];
        assert_eq!(group.kind, "file");
        assert_eq!(group.name, "src/main.rs");
        assert_eq!(group.node_ids, vec![first, second]);
    }

    #[tokio::test]
    async fn counts_isolated_nodes_and_excludes_connected_ones() {
        let (service, repository) = service().await;
        let goal_kind = repository
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "goal")
            .expect("seeded goal kind");
        let task_kind = repository
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "task")
            .expect("seeded task kind");
        let part_of = repository
            .list_edge_types()
            .await
            .unwrap()
            .into_iter()
            .find(|edge_type| edge_type.name == "part_of")
            .expect("seeded part_of edge type");

        let goal = node(&repository, goal_kind.id, "ship health report").await;
        let task = node(&repository, task_kind.id, "write report").await;
        let lonely = node(&repository, task_kind.id, "orphan task").await;
        repository
            .add_edge(&Edge {
                source: task,
                destination: goal,
                edge_type_id: part_of.id,
            })
            .await
            .expect("add edge");

        let report = service.report().await.expect("report");
        assert_eq!(report.isolated_count, 1);
        assert_eq!(report.isolated.len(), 1);
        assert_eq!(report.isolated[0].id, lonely);
        assert_eq!(report.isolated[0].kind, "task");
        // Both endpoints of the edge are connected in one direction each.
        assert!(!report.isolated.iter().any(|node| node.id == task));
        assert!(!report.isolated.iter().any(|node| node.id == goal));
        assert_eq!(report.edge_count, 1);
    }

    #[tokio::test]
    async fn bounds_isolated_sample_while_reporting_true_total() {
        let (service, repository) = service().await;
        let bug_kind = repository
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "bug")
            .expect("seeded bug kind");
        for index in 0..5 {
            node(&repository, bug_kind.id, &format!("bug-{index}")).await;
        }

        let report = service.report_with_sample(2).await.expect("report");
        assert_eq!(report.isolated_count, 5);
        assert_eq!(report.isolated.len(), 2);
    }

    #[tokio::test]
    async fn summary_line_reports_the_same_numbers_as_the_report() {
        let (service, _) = service().await;
        let report = service.report().await.expect("report");
        let summary = report.summary_line();
        assert!(summary.contains(&format!("nodes={}", report.node_count)));
        assert!(summary.contains(&format!("duplicate_groups={}", report.duplicates.len())));
        assert!(summary.contains(&format!("isolated_nodes={}", report.isolated_count)));
    }
}
