use async_trait::async_trait;
use sqlx::{FromRow, Row, SqlitePool};

use crate::model::graph::{Edge, Node, Relation, SearchHit};

use super::error::RepositoryError;

const NODE_COLUMNS: &str = "id, kind, name, description, content";

#[async_trait]
pub trait GraphRepository: Send + Sync {
    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError>;
    async fn get_node(&self, id: i64) -> Result<Option<Node>, RepositoryError>;
    async fn delete_node(&self, id: i64) -> Result<bool, RepositoryError>;
    async fn list_nodes(&self) -> Result<Vec<Node>, RepositoryError>;

    async fn upsert_relation(&self, relation: &Relation) -> Result<Relation, RepositoryError>;
    async fn get_relation_by_name(&self, name: &str) -> Result<Option<Relation>, RepositoryError>;
    async fn list_relations(&self) -> Result<Vec<Relation>, RepositoryError>;

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
    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError> {
        if node.id == 0 {
            let row = sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (kind, name, description, content) VALUES (?, ?, ?, ?) \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(&node.kind)
            .bind(&node.name)
            .bind(&node.description)
            .bind(&node.content)
            .fetch_one(&self.pool)
            .await?;
            Ok(row)
        } else {
            let row = sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (id, kind, name, description, content) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, \
                 description = excluded.description, content = excluded.content \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(node.id)
            .bind(&node.kind)
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

    async fn upsert_relation(&self, relation: &Relation) -> Result<Relation, RepositoryError> {
        let row = sqlx::query_as::<_, Relation>(
            "INSERT INTO relation (name, description) VALUES (?, ?) \
             ON CONFLICT(name) DO UPDATE SET description = excluded.description \
             RETURNING id, name, description",
        )
        .bind(&relation.name)
        .bind(&relation.description)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_relation_by_name(&self, name: &str) -> Result<Option<Relation>, RepositoryError> {
        let row = sqlx::query_as::<_, Relation>(
            "SELECT id, name, description FROM relation WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_relations(&self) -> Result<Vec<Relation>, RepositoryError> {
        let rows = sqlx::query_as::<_, Relation>(
            "SELECT id, name, description FROM relation ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO edge (source, destination, relation_id) VALUES (?, ?, ?) \
             ON CONFLICT DO NOTHING",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.relation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_edge(&self, edge: &Edge) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "DELETE FROM edge WHERE source = ? AND destination = ? AND relation_id = ?",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.relation_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn neighbors(&self, source: i64) -> Result<Vec<(Edge, Node)>, RepositoryError> {
        let rows = sqlx::query(
            "SELECT e.source, e.destination, e.relation_id, \
             n.id, n.kind, n.name, n.description, n.content \
             FROM edge e JOIN node n ON n.id = e.destination \
             WHERE e.source = ? ORDER BY e.relation_id, e.destination",
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
            "SELECT n.id, n.kind, n.name, n.description, n.content, bm25(node_fts) AS score \
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
