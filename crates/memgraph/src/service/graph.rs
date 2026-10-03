use std::sync::Arc;

use crate::{
    model::{Edge, EdgeType, Node, NodeKind, SearchHit},
    repository::graph::GraphRepository,
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

    pub async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, ServiceError> {
        self.repository
            .create_node_kind(node_kind)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_node_kinds(&self) -> Result<Vec<NodeKind>, ServiceError> {
        self.repository
            .list_node_kinds()
            .await
            .map_err(ServiceError::from)
    }

    pub async fn create_edge_type(&self, edge_type: &EdgeType) -> Result<EdgeType, ServiceError> {
        self.repository
            .create_edge_type(edge_type)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn list_edge_types(&self) -> Result<Vec<EdgeType>, ServiceError> {
        self.repository
            .list_edge_types()
            .await
            .map_err(ServiceError::from)
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

#[cfg(test)]
mod tests {
    use crate::{
        database::{create_sqlite_pool, migrate},
        repository::graph::SqliteGraphRepository,
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

    fn node_kind(name: &str) -> NodeKind {
        NodeKind {
            id: 0,
            name: name.to_string(),
            description: String::new(),
        }
    }

    fn edge_type(name: &str, description: &str) -> EdgeType {
        EdgeType {
            id: 0,
            name: name.to_string(),
            description: description.to_string(),
        }
    }

    fn node(kind_id: i64, name: &str, content: &str) -> Node {
        Node {
            id: 0,
            kind_id,
            name: name.to_string(),
            description: String::new(),
            content: content.to_string(),
        }
    }

    async fn seed_person(service: &GraphService) -> i64 {
        service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn creates_and_lists_node_kinds() {
        let service = service().await;
        let created = service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert_eq!(service.list_node_kinds().await.unwrap(), vec![created]);
    }

    #[tokio::test]
    async fn rejects_duplicate_node_kind() {
        let service = service().await;
        service
            .create_node_kind(&node_kind("person"))
            .await
            .unwrap();
        assert!(matches!(
            service.create_node_kind(&node_kind("person")).await,
            Err(ServiceError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn creates_and_lists_edge_types() {
        let service = service().await;
        let created = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert_eq!(service.list_edge_types().await.unwrap(), vec![created]);
    }

    #[tokio::test]
    async fn assigns_id_on_insert() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", ""))
            .await
            .unwrap();
        assert!(created.id > 0);
        assert_eq!(service.get_node(created.id).await.unwrap(), created);
    }

    #[tokio::test]
    async fn rejects_node_with_unknown_kind() {
        let service = service().await;
        assert!(matches!(
            service.upsert_node(&node(404, "Ada", "")).await,
            Err(ServiceError::UnprocessableEntity(_))
        ));
    }

    #[tokio::test]
    async fn keeps_id_on_conflict() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", "a"))
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
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", ""))
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
    async fn traverses_directed_edges() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&node(kind_id, "Ada", ""))
            .await
            .unwrap();
        let bob = service
            .upsert_node(&node(kind_id, "Bob", ""))
            .await
            .unwrap();
        let knows = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        let edge = Edge {
            source: ada.id,
            destination: bob.id,
            edge_type_id: knows.id,
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
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&node(kind_id, "Ada Lovelace", "pioneer of computing"))
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
