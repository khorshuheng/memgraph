use async_trait::async_trait;
use sqlx::{FromRow, Row, SqlitePool};

use crate::model::{Edge, EdgeType, Node, NodeKind, SearchHit};

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

    async fn search(&self, fts_query: &str) -> Result<Vec<SearchHit>, RepositoryError>;
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

    async fn search(&self, fts_query: &str) -> Result<Vec<SearchHit>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT n.id, n.kind_id, n.name, n.description, n.content, bm25(node_fts) AS score \
             FROM node_fts JOIN node n ON n.id = node_fts.rowid \
             WHERE node_fts MATCH ? ORDER BY score, n.id",
        )
        .bind(fts_query)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(SearchHit {
                    node: Node::from_row(row)?,
                    score: row.try_get("score")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
    }
}
