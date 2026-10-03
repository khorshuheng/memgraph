use std::sync::Arc;

use crate::{
    model::graph::{Edge, Node, Relation, SearchHit},
    repository::{error::RepositoryError, graph::GraphRepository},
};

use super::error::ServiceError;

#[derive(Clone)]
pub struct GraphService {
    repository: Arc<dyn GraphRepository>,
}

impl GraphService {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }

    pub async fn upsert_node(&self, node: &Node) -> Result<Node, ServiceError> {
        self.repository
            .upsert_node(node)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn get_node(&self, id: i64) -> Result<Node, ServiceError> {
        match self.repository.get_node(id).await {
            Ok(Some(node)) => Ok(node),
            Ok(None) => Err(ServiceError::EntityNotFound),
            Err(err) => Err(ServiceError::from(err)),
        }
    }

    pub async fn delete_node(&self, id: i64) -> Result<(), ServiceError> {
        match self.repository.delete_node(id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ServiceError::EntityNotFound),
            Err(err) => Err(ServiceError::from(err)),
        }
    }

    pub async fn list_nodes(&self) -> Result<Vec<Node>, ServiceError> {
        self.repository
            .list_nodes()
            .await
            .map_err(ServiceError::from)
    }

    pub async fn upsert_relation(&self, relation: &Relation) -> Result<Relation, ServiceError> {
        self.repository
            .upsert_relation(relation)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_relations(&self) -> Result<Vec<Relation>, ServiceError> {
        self.repository
            .list_relations()
            .await
            .map_err(ServiceError::from)
    }

    pub async fn add_edge(&self, edge: &Edge) -> Result<(), ServiceError> {
        self.repository
            .add_edge(edge)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn remove_edge(&self, edge: &Edge) -> Result<(), ServiceError> {
        match self.repository.remove_edge(edge).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ServiceError::EntityNotFound),
            Err(err) => Err(ServiceError::from(err)),
        }
    }

    pub async fn neighbors(&self, source: i64) -> Result<Vec<(Edge, Node)>, ServiceError> {
        self.repository
            .neighbors(source)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn search(&self, fts_query: &str) -> Result<Vec<SearchHit>, ServiceError> {
        self.repository
            .search(fts_query)
            .await
            .map_err(ServiceError::from)
    }
}

impl From<RepositoryError> for ServiceError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::RowNotFound => ServiceError::EntityNotFound,
            RepositoryError::UniqueConstraintViolation(err) => ServiceError::Conflict(err),
            err => ServiceError::InternalError(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        repository::graph::SqliteGraphRepository,
        state::{create_sqlite_pool, migrate},
    };

    use super::*;

    async fn service() -> GraphService {
        let config = crate::config::DatabaseConfig {
            path: ":memory:".to_string(),
            max_connections: 1,
        };
        let pool = create_sqlite_pool(&config)
            .await
            .expect("failed to create database connection pool");
        migrate(&pool).await.expect("failed to migrate database");
        GraphService::new(Arc::new(SqliteGraphRepository::new(pool)))
    }

    fn node(kind: &str, name: &str, content: &str) -> Node {
        Node {
            id: 0,
            kind: kind.to_string(),
            name: name.to_string(),
            description: String::new(),
            content: content.to_string(),
        }
    }

    fn relation(name: &str, description: &str) -> Relation {
        Relation {
            id: 0,
            name: name.to_string(),
            description: description.to_string(),
        }
    }

    #[tokio::test]
    async fn assigns_id_on_insert() {
        let service = service().await;
        let created = service
            .upsert_node(&node("person", "Ada", ""))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert_eq!(service.get_node(created.id).await.unwrap(), created);
    }

    #[tokio::test]
    async fn keeps_id_on_conflict() {
        let service = service().await;
        let created = service
            .upsert_node(&node("person", "Ada", "a"))
            .await
            .unwrap();
        let mut changed = created.clone();
        changed.content = "b".to_string();
        let updated = service.upsert_node(&changed).await.unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.content, "b");
        assert_eq!(service.list_nodes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deletes_node_and_fts_row() {
        let service = service().await;
        let created = service
            .upsert_node(&node("person", "Ada", ""))
            .await
            .unwrap();
        service.delete_node(created.id).await.unwrap();
        assert_eq!(
            service.delete_node(created.id).await.unwrap_err(),
            ServiceError::EntityNotFound
        );
        assert!(service.search("ada").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn upsert_relation_is_idempotent_by_name() {
        let service = service().await;
        let first = service
            .upsert_relation(&relation("knows", ""))
            .await
            .unwrap();
        let second = service
            .upsert_relation(&relation("knows", "knows someone"))
            .await
            .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.description, "knows someone");
        assert_eq!(service.list_relations().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn traverses_directed_edges() {
        let service = service().await;
        let ada = service
            .upsert_node(&node("person", "Ada", ""))
            .await
            .unwrap();
        let bob = service
            .upsert_node(&node("person", "Bob", ""))
            .await
            .unwrap();
        let knows = service
            .upsert_relation(&relation("knows", ""))
            .await
            .unwrap();
        let edge = Edge {
            source: ada.id,
            destination: bob.id,
            relation_id: knows.id,
        };
        service.add_edge(&edge).await.unwrap();
        service.add_edge(&edge).await.unwrap();
        let neighbors = service.neighbors(ada.id).await.unwrap();
        assert_eq!(neighbors, vec![(edge.clone(), bob.clone())]);
        assert!(service.neighbors(bob.id).await.unwrap().is_empty());
        service.remove_edge(&edge).await.unwrap();
        assert_eq!(
            service.remove_edge(&edge).await.unwrap_err(),
            ServiceError::EntityNotFound
        );
        assert!(service.neighbors(ada.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_matches_and_reflects_updates() {
        let service = service().await;
        let ada = service
            .upsert_node(&node("person", "Ada Lovelace", "pioneer of computing"))
            .await
            .unwrap();
        let hits = service.search("computing").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node.id, ada.id);

        let mut changed = ada.clone();
        changed.content = "mathematician".to_string();
        service.upsert_node(&changed).await.unwrap();
        assert!(service.search("computing").await.unwrap().is_empty());
        assert_eq!(service.search("mathematician").await.unwrap().len(), 1);
    }
}
