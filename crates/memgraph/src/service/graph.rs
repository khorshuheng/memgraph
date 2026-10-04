use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::{
    config::SearchConfig,
    model::{Edge, EdgeType, Node, NodeKind, SearchHit, SegmentMatches},
    repository::graph::GraphRepository,
};

use super::error::ServiceError;

#[derive(Clone)]
pub struct GraphService {
    repository: Arc<dyn GraphRepository>,
    search_config: SearchConfig,
}

impl GraphService {
    pub fn new(repository: Arc<dyn GraphRepository>, search_config: SearchConfig) -> Self {
        Self {
            repository,
            search_config,
        }
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

    pub async fn search(&self, query: &str) -> Result<Vec<SegmentMatches>, ServiceError> {
        let mut segments = Vec::new();
        for segment in split_segments(query, &self.search_config.split_characters()) {
            let hits = self.score_segment(&segment).await?;
            if hits.is_empty() {
                continue;
            }
            let total_matches = hits.len();
            let matches = hits
                .into_iter()
                .take(self.search_config.per_segment_limit)
                .collect();
            segments.push(SegmentMatches {
                segment,
                matches,
                total_matches,
            });
        }
        Ok(segments)
    }

    pub async fn search_segment(
        &self,
        segment: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, ServiceError> {
        let hits = self.score_segment(segment).await?;
        Ok(hits.into_iter().skip(offset).take(limit).collect())
    }

    pub fn summary_chars(&self) -> usize {
        self.search_config.summary_chars
    }

    pub fn default_segment_limit(&self) -> usize {
        self.search_config.per_segment_limit
    }

    async fn score_segment(&self, segment: &str) -> Result<Vec<SearchHit>, ServiceError> {
        let considered = self.considered_terms(segment).await?;
        if considered.is_empty() {
            return Ok(Vec::new());
        }
        let denominator: f64 = considered.iter().map(|(_, weight)| weight).sum();
        let weights: HashMap<&str, f64> = considered
            .iter()
            .map(|(term, weight)| (term.as_str(), *weight))
            .collect();
        let terms: Vec<String> = considered.iter().map(|(term, _)| term.clone()).collect();
        let fts_query = build_fts_query(&terms);
        let candidates = self.repository.search_descriptions(&fts_query).await?;

        let mut hits = Vec::new();
        for node in candidates {
            let tokens: HashSet<String> = tokenize(&node.description).into_iter().collect();
            let matched: Vec<String> = terms
                .iter()
                .filter(|term| tokens.contains(*term))
                .cloned()
                .collect();
            let mass: f64 = matched
                .iter()
                .map(|term| weights.get(term.as_str()).copied().unwrap_or(0.0))
                .sum();
            let confidence = if denominator > 0.0 {
                mass / denominator
            } else {
                0.0
            };
            if matched.len() < self.search_config.min_matched_terms {
                continue;
            }
            if mass < self.search_config.min_matched_idf {
                continue;
            }
            if confidence < self.search_config.confidence_threshold {
                continue;
            }
            hits.push(SearchHit {
                relations: self.repository.relation_summaries(node.id).await?,
                node,
                confidence,
                matched_terms: matched,
            });
        }
        hits.sort_by(|left, right| {
            right
                .confidence
                .partial_cmp(&left.confidence)
                .unwrap_or(Ordering::Equal)
                .then(left.node.id.cmp(&right.node.id))
        });
        Ok(hits)
    }

    async fn considered_terms(&self, segment: &str) -> Result<Vec<(String, f64)>, ServiceError> {
        let mut resolved: BTreeSet<String> = BTreeSet::new();
        let mut seen: HashSet<String> = HashSet::new();
        for token in tokenize(segment) {
            if !seen.insert(token.clone()) {
                continue;
            }
            for term in self.resolve_token(&token).await? {
                resolved.insert(term);
            }
        }
        if resolved.is_empty() {
            return Ok(Vec::new());
        }
        let node_count = self.repository.count_nodes().await? as f64;
        let mut weighted = Vec::new();
        for term in resolved {
            let frequency = self.repository.term_document_frequency(&term).await? as f64;
            weighted.push((term, inverse_document_frequency(node_count, frequency)));
        }
        weighted.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then(left.0.cmp(&right.0))
        });
        weighted.truncate(self.search_config.max_terms);
        Ok(weighted)
    }

    async fn resolve_token(&self, token: &str) -> Result<Vec<String>, ServiceError> {
        let min_length = self.search_config.prefix_min_length;
        let characters: Vec<char> = token.chars().collect();
        let mut terms: BTreeSet<String> = BTreeSet::new();
        if self.repository.term_exists(token).await? {
            terms.insert(token.to_string());
        }
        if characters.len() >= min_length {
            for term in self.repository.terms_with_prefix(token).await? {
                terms.insert(term);
            }
        }
        for length in min_length..characters.len() {
            let candidate: String = characters[..length].iter().collect();
            if self.repository.term_exists(&candidate).await? {
                terms.insert(candidate);
            }
        }
        Ok(terms.into_iter().collect())
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

fn inverse_document_frequency(node_count: f64, document_frequency: f64) -> f64 {
    (1.0 + node_count / document_frequency).ln()
}

fn split_segments(text: &str, separators: &[char]) -> Vec<String> {
    text.split(|character| separators.contains(&character))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn build_fts_query(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<String>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use crate::{
        database::{create_sqlite_pool, migrate},
        model::RelationDirection,
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
        GraphService::new(
            Arc::new(SqliteGraphRepository::new(pool)),
            test_search_config(),
        )
    }

    fn test_search_config() -> crate::config::SearchConfig {
        crate::config::SearchConfig {
            min_matched_idf: 0.0,
            ..crate::config::SearchConfig::default()
        }
    }

    fn node_kind(name: &str) -> NodeKind {
        NodeKind {
            id: 0,
            name: name.to_string(),
            description: String::new(),
            updated_at: String::new(),
        }
    }

    fn edge_type(name: &str, description: &str) -> EdgeType {
        EdgeType {
            id: 0,
            name: name.to_string(),
            description: description.to_string(),
            updated_at: String::new(),
        }
    }

    fn node(kind_id: i64, name: &str, content: &str) -> Node {
        Node {
            id: 0,
            kind_id,
            name: name.to_string(),
            description: String::new(),
            content: content.to_string(),
            updated_at: String::new(),
        }
    }

    fn described_node(kind_id: i64, name: &str, description: &str) -> Node {
        Node {
            id: 0,
            kind_id,
            name: name.to_string(),
            description: description.to_string(),
            content: String::new(),
            updated_at: String::new(),
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
        assert!(service.list_node_kinds().await.unwrap().contains(&created));
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
        assert!(service.list_edge_types().await.unwrap().contains(&created));
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
    async fn preserves_updated_at_when_node_is_unchanged() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", "a"))
            .await
            .unwrap();
        assert!(!created.updated_at.is_empty());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let updated = service.upsert_node(&created).await.unwrap();
        assert_eq!(updated.updated_at, created.updated_at);
    }

    #[tokio::test]
    async fn bumps_updated_at_when_node_changes() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&node(kind_id, "Ada", "a"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let mut changed = created.clone();
        changed.content = "b".to_string();
        let updated = service.upsert_node(&changed).await.unwrap();
        assert_ne!(updated.updated_at, created.updated_at);
    }

    #[tokio::test]
    async fn deletes_node_and_fts_row() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let created = service
            .upsert_node(&described_node(kind_id, "Ada", "ada lovelace"))
            .await
            .unwrap();
        assert_eq!(service.search("ada lovelace").await.unwrap().len(), 1);
        service.delete_node(created.id).await.unwrap();
        assert_eq!(
            service.delete_node(created.id).await.unwrap_err(),
            ServiceError::EntityNotFound
        );
        assert!(service.search("ada lovelace").await.unwrap().is_empty());
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
        let mut edge = Edge {
            source: ada.id,
            destination: bob.id,
            edge_type_id: knows.id,
            created_at: String::new(),
        };
        service.add_edge(&edge).await.unwrap();
        service.add_edge(&edge).await.unwrap();
        let neighbors = service.neighbors(ada.id).await.unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].1, bob);
        edge.created_at = neighbors[0].0.created_at.clone();
        assert!(!edge.created_at.is_empty());
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
            .upsert_node(&described_node(
                kind_id,
                "Ada Lovelace",
                "pioneer of computing",
            ))
            .await
            .unwrap();
        let segments = service.search("pioneer computing").await.unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].matches.len(), 1);
        assert_eq!(segments[0].matches[0].node.id, ada.id);
        assert!(segments[0].matches[0].confidence >= 0.99);

        let mut changed = ada.clone();
        changed.description = "mathematician and logician".to_string();
        service.upsert_node(&changed).await.unwrap();
        assert!(
            service
                .search("pioneer computing")
                .await
                .unwrap()
                .is_empty()
        );
        let segments = service.search("mathematician logician").await.unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].matches[0].node.id, ada.id);
    }

    #[tokio::test]
    async fn gates_out_single_term_overlap() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        service
            .upsert_node(&described_node(kind_id, "Ledger", "owned debt record"))
            .await
            .unwrap();
        assert!(service.search("debt repayment").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn splits_multi_topic_prompt() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let composer = service
            .upsert_node(&described_node(kind_id, "Composer", "musical composition"))
            .await
            .unwrap();
        let database = service
            .upsert_node(&described_node(kind_id, "Database", "query storage engine"))
            .await
            .unwrap();
        let segments = service
            .search("musical composition? query storage engine")
            .await
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].matches[0].node.id, composer.id);
        assert_eq!(segments[1].matches[0].node.id, database.id);
    }

    #[tokio::test]
    async fn search_hit_includes_topology_probe() {
        let service = service().await;
        let kind_id = seed_person(&service).await;
        let ada = service
            .upsert_node(&described_node(kind_id, "Ada", "ada lovelace"))
            .await
            .unwrap();
        let bob = service
            .upsert_node(&described_node(kind_id, "Bob", "ada lovelace"))
            .await
            .unwrap();
        let knows = service
            .create_edge_type(&edge_type("knows", ""))
            .await
            .unwrap();
        service
            .add_edge(&Edge {
                source: ada.id,
                destination: bob.id,
                edge_type_id: knows.id,
                created_at: String::new(),
            })
            .await
            .unwrap();
        let segments = service.search("ada lovelace").await.unwrap();
        let ada_hit = segments[0]
            .matches
            .iter()
            .find(|hit| hit.node.id == ada.id)
            .unwrap();
        assert_eq!(ada_hit.relations.len(), 1);
        assert_eq!(ada_hit.relations[0].direction, RelationDirection::Outgoing);
        assert_eq!(ada_hit.relations[0].edge_type, "knows");
        assert_eq!(ada_hit.relations[0].count, 1);
        let bob_hit = segments[0]
            .matches
            .iter()
            .find(|hit| hit.node.id == bob.id)
            .unwrap();
        assert_eq!(bob_hit.relations[0].direction, RelationDirection::Ingoing);
    }
}
