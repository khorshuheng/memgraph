use async_trait::async_trait;
use sqlx::{FromRow, SqlitePool};

use crate::model::{Edge, EdgeType, Node, NodeKind, RelationDirection, RelationSummary};

use super::error::RepositoryError;

const NODE_KIND_COLUMNS: &str = "id, name, description";
const EDGE_TYPE_COLUMNS: &str = "id, name, description";
const NODE_COLUMNS: &str = "id, kind_id, name, description, content";

#[async_trait]
pub trait GraphRepository: Send + Sync {
    async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, RepositoryError>;
    async fn list_node_kinds(&self) -> Result<Vec<NodeKind>, RepositoryError>;

    async fn create_edge_type(&self, edge_type: &EdgeType) -> Result<EdgeType, RepositoryError>;
    async fn list_edge_types(&self) -> Result<Vec<EdgeType>, RepositoryError>;

    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError>;
    async fn get_node(&self, id: i64) -> Result<Option<Node>, RepositoryError>;
    async fn delete_node(&self, id: i64) -> Result<bool, RepositoryError>;
    async fn list_nodes(&self) -> Result<Vec<Node>, RepositoryError>;

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError>;
    async fn remove_edge(&self, edge: &Edge) -> Result<bool, RepositoryError>;
    async fn neighbors(&self, source: i64) -> Result<Vec<(Edge, Node)>, RepositoryError>;

    async fn count_nodes(&self) -> Result<i64, RepositoryError>;
    async fn term_document_frequency(&self, term: &str) -> Result<i64, RepositoryError>;
    async fn terms_with_prefix(&self, prefix: &str) -> Result<Vec<String>, RepositoryError>;
    async fn term_exists(&self, term: &str) -> Result<bool, RepositoryError>;
    async fn search_descriptions(&self, fts_query: &str) -> Result<Vec<Node>, RepositoryError>;
    async fn relation_summaries(
        &self,
        node_id: i64,
    ) -> Result<Vec<RelationSummary>, RepositoryError>;
}

#[derive(Clone)]
pub struct SqliteGraphRepository {
    pool: SqlitePool,
}

impl SqliteGraphRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GraphRepository for SqliteGraphRepository {
    async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, RepositoryError> {
        let row = sqlx::query_as::<_, NodeKind>(&format!(
            "INSERT INTO node_kind (name, description) VALUES (?, ?) RETURNING {NODE_KIND_COLUMNS}"
        ))
        .bind(&node_kind.name)
        .bind(&node_kind.description)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_node_kinds(&self) -> Result<Vec<NodeKind>, RepositoryError> {
        let rows = sqlx::query_as::<_, NodeKind>(&format!(
            "SELECT {NODE_KIND_COLUMNS} FROM node_kind ORDER BY id"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn create_edge_type(&self, edge_type: &EdgeType) -> Result<EdgeType, RepositoryError> {
        let row = sqlx::query_as::<_, EdgeType>(&format!(
            "INSERT INTO edge_type (name, description) VALUES (?, ?) RETURNING {EDGE_TYPE_COLUMNS}"
        ))
        .bind(&edge_type.name)
        .bind(&edge_type.description)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_edge_types(&self) -> Result<Vec<EdgeType>, RepositoryError> {
        let rows = sqlx::query_as::<_, EdgeType>(&format!(
            "SELECT {EDGE_TYPE_COLUMNS} FROM edge_type ORDER BY id"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError> {
        if node.id == 0 {
            let row = sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (kind_id, name, description, content) VALUES (?, ?, ?, ?) \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(node.kind_id)
            .bind(&node.name)
            .bind(&node.description)
            .bind(&node.content)
            .fetch_one(&self.pool)
            .await?;
            Ok(row)
        } else {
            let row = sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (id, kind_id, name, description, content) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET kind_id = excluded.kind_id, name = excluded.name, \
                 description = excluded.description, content = excluded.content \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(node.id)
            .bind(node.kind_id)
            .bind(&node.name)
            .bind(&node.description)
            .bind(&node.content)
            .fetch_one(&self.pool)
            .await?;
            Ok(row)
        }
    }

    async fn get_node(&self, id: i64) -> Result<Option<Node>, RepositoryError> {
        let row =
            sqlx::query_as::<_, Node>(&format!("SELECT {NODE_COLUMNS} FROM node WHERE id = ?"))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row)
    }

    async fn delete_node(&self, id: i64) -> Result<bool, RepositoryError> {
        let result = sqlx::query("DELETE FROM node WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_nodes(&self) -> Result<Vec<Node>, RepositoryError> {
        let rows =
            sqlx::query_as::<_, Node>(&format!("SELECT {NODE_COLUMNS} FROM node ORDER BY id"))
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO edge (source, destination, edge_type_id) VALUES (?, ?, ?) \
             ON CONFLICT DO NOTHING",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.edge_type_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_edge(&self, edge: &Edge) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "DELETE FROM edge WHERE source = ? AND destination = ? AND edge_type_id = ?",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.edge_type_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn neighbors(&self, source: i64) -> Result<Vec<(Edge, Node)>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT e.source, e.destination, e.edge_type_id, \
             n.id, n.kind_id, n.name, n.description, n.content \
             FROM edge e JOIN node n ON n.id = e.destination \
             WHERE e.source = ? ORDER BY e.edge_type_id, e.destination",
        )
        .bind(source)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| Ok((Edge::from_row(row)?, Node::from_row(row)?)))
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
    }

    async fn count_nodes(&self) -> Result<i64, RepositoryError> {
        let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM node")
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    async fn term_document_frequency(&self, term: &str) -> Result<i64, RepositoryError> {
        let frequency =
            sqlx::query_scalar::<_, i64>("SELECT doc FROM node_fts_vocab WHERE term = ?")
                .bind(term)
                .fetch_optional(&self.pool)
                .await?;
        Ok(frequency.unwrap_or(0))
    }

    async fn terms_with_prefix(&self, prefix: &str) -> Result<Vec<String>, RepositoryError> {
        let pattern = format!("{}%", escape_like_pattern(prefix));
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT term FROM node_fts_vocab WHERE term LIKE ? ESCAPE '\\' ORDER BY term",
        )
        .bind(pattern)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn term_exists(&self, term: &str) -> Result<bool, RepositoryError> {
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT 1 FROM node_fts_vocab WHERE term = ? LIMIT 1")
                .bind(term)
                .fetch_optional(&self.pool)
                .await?
                .is_some();
        Ok(exists)
    }

    async fn search_descriptions(&self, fts_query: &str) -> Result<Vec<Node>, RepositoryError> {
        let rows = sqlx::query_as::<_, Node>(
            "SELECT n.id, n.kind_id, n.name, n.description, n.content \
             FROM node_fts JOIN node n ON n.id = node_fts.rowid \
             WHERE node_fts MATCH ? ORDER BY bm25(node_fts), n.id",
        )
        .bind(fts_query)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn relation_summaries(
        &self,
        node_id: i64,
    ) -> Result<Vec<RelationSummary>, RepositoryError> {
        let rows = sqlx::query_as::<_, (i64, i64, String, i64)>(
            "SELECT 0 AS direction, e.edge_type_id, t.name, count(*) AS count \
             FROM edge e JOIN edge_type t ON t.id = e.edge_type_id \
             WHERE e.source = ? GROUP BY e.edge_type_id, t.name \
             UNION ALL \
             SELECT 1, e.edge_type_id, t.name, count(*) \
             FROM edge e JOIN edge_type t ON t.id = e.edge_type_id \
             WHERE e.destination = ? GROUP BY e.edge_type_id, t.name \
             ORDER BY direction, edge_type_id",
        )
        .bind(node_id)
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(direction, edge_type_id, edge_type, count)| RelationSummary {
                    direction: if direction == 0 {
                        RelationDirection::Outgoing
                    } else {
                        RelationDirection::Ingoing
                    },
                    edge_type_id,
                    edge_type,
                    count,
                },
            )
            .collect())
    }
}

fn escape_like_pattern(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
