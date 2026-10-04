use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::{FromRow, Row, SqlitePool};

use crate::model::{
    AccessAction, Edge, EdgeType, EdgeTypeUsage, IsolatedNode, KindUsage, Neighbor,
    NeighborDirection, Node, NodeFilter, NodeKind, NodeRef, PendingEdge, ReachFilter,
    RelationDirection, RelationSummary, SearchScope,
};

use super::error::RepositoryError;

const NODE_KIND_COLUMNS: &str = "id, name, description, updated_at";
const EDGE_TYPE_COLUMNS: &str =
    "id, name, description, resolves, single_outgoing, source_kinds, destination_kinds, updated_at";
const NODE_COLUMNS: &str = "id, kind_id, name, description, content, updated_at";
const OUTGOING_NEIGHBORS: &str = "SELECT e.source, e.destination, e.edge_type_id, e.created_at, \
    n.id, n.kind_id, n.name, n.description, n.content, n.updated_at \
    FROM edge e JOIN node n ON n.id = e.destination \
    WHERE e.source = ? \
      AND (? IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?)) \
    ORDER BY e.edge_type_id, e.destination";
const INCOMING_NEIGHBORS: &str = "SELECT e.source, e.destination, e.edge_type_id, e.created_at, \
    n.id, n.kind_id, n.name, n.description, n.content, n.updated_at \
    FROM edge e JOIN node n ON n.id = e.source \
    WHERE e.destination = ? \
      AND (? IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?)) \
    ORDER BY e.edge_type_id, e.source";
const REACH_CTE: &str = "WITH RECURSIVE reach(id) AS ( \
    SELECT e.source FROM edge e WHERE e.destination = ? \
      AND (? IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?)) \
    UNION \
    SELECT e.destination FROM edge e WHERE e.source = ? \
      AND (? IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?)) \
    UNION \
    SELECT e.source FROM edge e JOIN reach r ON e.destination = r.id \
      WHERE e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?) \
  ) ";
const ANCHORED_CTE: &str = "WITH RECURSIVE anchored(id) AS ( \
    SELECT e.source FROM edge e \
      WHERE e.edge_type_id = (SELECT id FROM edge_type WHERE name = 'belongs_to') \
    UNION \
    SELECT e.source FROM edge e JOIN anchored a ON e.destination = a.id \
      WHERE e.edge_type_id = (SELECT id FROM edge_type WHERE name = 'part_of') \
  ) ";
const ANCESTRY_CTE: &str = "WITH RECURSIVE ancestry(id) AS ( \
    SELECT ? \
    UNION \
    SELECT e.destination FROM edge e JOIN ancestry a ON e.source = a.id \
      WHERE e.edge_type_id = (SELECT id FROM edge_type WHERE name = 'part_of') \
  ) ";

fn scope_clause(scope: SearchScope) -> &'static str {
    match scope {
        SearchScope::All => "",
        SearchScope::Active => {
            " AND NOT EXISTS(SELECT 1 FROM edge e JOIN edge_type t ON t.id = e.edge_type_id \
             WHERE e.destination = n.id AND t.resolves = 1)"
        }
        SearchScope::Resolved => {
            " AND EXISTS(SELECT 1 FROM edge e JOIN edge_type t ON t.id = e.edge_type_id \
             WHERE e.destination = n.id AND t.resolves = 1)"
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct NodeAccessRecord {
    pub node_id: i64,
    pub access_id: i64,
}

#[async_trait]
pub trait GraphRepository: Send + Sync {
    async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, RepositoryError>;
    async fn list_node_kinds(&self) -> Result<Vec<NodeKind>, RepositoryError>;

    async fn create_edge_type(&self, edge_type: &EdgeType) -> Result<EdgeType, RepositoryError>;
    async fn list_edge_types(&self) -> Result<Vec<EdgeType>, RepositoryError>;

    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError>;
    async fn insert_nodes_with_edges(
        &self,
        nodes: &[Node],
        edges: &[PendingEdge],
    ) -> Result<Vec<Node>, RepositoryError>;
    async fn get_node(&self, id: i64) -> Result<Option<Node>, RepositoryError>;
    async fn delete_node(&self, id: i64) -> Result<bool, RepositoryError>;
    async fn list_nodes(&self, filter: &NodeFilter) -> Result<Vec<Node>, RepositoryError>;
    async fn node_kinds_for_ids(&self, ids: &[i64]) -> Result<Vec<(i64, String)>, RepositoryError>;

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError>;
    async fn remove_edge(&self, edge: &Edge) -> Result<bool, RepositoryError>;
    async fn neighbors(
        &self,
        node_id: i64,
        direction: NeighborDirection,
        edge_type: Option<&str>,
    ) -> Result<Vec<Neighbor>, RepositoryError>;

    async fn count_nodes(&self) -> Result<i64, RepositoryError>;
    async fn term_document_frequency(&self, term: &str) -> Result<i64, RepositoryError>;
    async fn terms_with_prefix(&self, prefix: &str) -> Result<Vec<String>, RepositoryError>;
    async fn term_exists(&self, term: &str) -> Result<bool, RepositoryError>;
    async fn search_descriptions(
        &self,
        fts_query: &str,
        reach: &ReachFilter,
    ) -> Result<Vec<(Node, bool)>, RepositoryError>;
    async fn relation_summaries(
        &self,
        node_id: i64,
    ) -> Result<Vec<RelationSummary>, RepositoryError>;

    async fn kind_usage(&self) -> Result<Vec<KindUsage>, RepositoryError>;
    async fn edge_type_usage(&self) -> Result<Vec<EdgeTypeUsage>, RepositoryError>;
    async fn duplicate_node_rows(&self)
    -> Result<Vec<(i64, String, String, i64)>, RepositoryError>;
    async fn isolated_node_count(&self) -> Result<i64, RepositoryError>;
    async fn isolated_nodes(&self, limit: i64) -> Result<Vec<IsolatedNode>, RepositoryError>;
    async fn is_anchored(&self, node_id: i64) -> Result<bool, RepositoryError>;
    async fn outgoing_edge_targets(
        &self,
        source: i64,
        edge_type_id: i64,
    ) -> Result<Vec<i64>, RepositoryError>;
    async fn unanchored_work_count(&self) -> Result<i64, RepositoryError>;
    async fn unanchored_work(&self, limit: i64) -> Result<Vec<IsolatedNode>, RepositoryError>;
    async fn record_node_accesses(
        &self,
        session_id: &str,
        node_ids: &[i64],
        action: AccessAction,
        query_term: Option<&str>,
    ) -> Result<Vec<NodeAccessRecord>, RepositoryError>;
    async fn set_node_access_relevance(
        &self,
        session_id: &str,
        access_id: i64,
        relevant: bool,
    ) -> Result<bool, RepositoryError>;
    async fn prune_node_access(&self, retention_days: u64) -> Result<u64, RepositoryError>;
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
            "INSERT INTO edge_type (name, description, resolves, single_outgoing, source_kinds, \
             destination_kinds) VALUES (?, ?, ?, ?, ?, ?) RETURNING {EDGE_TYPE_COLUMNS}"
        ))
        .bind(&edge_type.name)
        .bind(&edge_type.description)
        .bind(edge_type.resolves)
        .bind(edge_type.single_outgoing)
        .bind(&edge_type.source_kinds)
        .bind(&edge_type.destination_kinds)
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
                 description = excluded.description, content = excluded.content, \
                 updated_at = CASE WHEN node.kind_id = excluded.kind_id \
                   AND node.name = excluded.name AND node.description = excluded.description \
                   AND node.content = excluded.content THEN node.updated_at ELSE excluded.updated_at END \
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

    async fn insert_nodes_with_edges(
        &self,
        nodes: &[Node],
        edges: &[PendingEdge],
    ) -> Result<Vec<Node>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let mut created = Vec::with_capacity(nodes.len());
        let mut ids = Vec::with_capacity(nodes.len());
        for node in nodes {
            let row = if node.id == 0 {
                sqlx::query_as::<_, Node>(&format!(
                    "INSERT INTO node (kind_id, name, description, content) VALUES (?, ?, ?, ?) \
                     RETURNING {NODE_COLUMNS}"
                ))
                .bind(node.kind_id)
                .bind(&node.name)
                .bind(&node.description)
                .bind(&node.content)
                .fetch_one(&mut *transaction)
                .await?
            } else {
                sqlx::query_as::<_, Node>(&format!(
                    "INSERT INTO node (id, kind_id, name, description, content) \
                     VALUES (?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET kind_id = excluded.kind_id, \
                     name = excluded.name, description = excluded.description, \
                     content = excluded.content, updated_at = CASE WHEN node.kind_id = excluded.kind_id \
                       AND node.name = excluded.name AND node.description = excluded.description \
                       AND node.content = excluded.content THEN node.updated_at ELSE excluded.updated_at END \
                     RETURNING {NODE_COLUMNS}"
                ))
                .bind(node.id)
                .bind(node.kind_id)
                .bind(&node.name)
                .bind(&node.description)
                .bind(&node.content)
                .fetch_one(&mut *transaction)
                .await?
            };
            ids.push(row.id);
            created.push(row);
        }
        let generated: HashSet<i64> = ids.iter().copied().collect();
        for edge in edges {
            let source = resolve_node_ref(edge.source, &ids, &generated)?;
            let destination = resolve_node_ref(edge.destination, &ids, &generated)?;
            sqlx::query(
                "INSERT INTO edge (source, destination, edge_type_id) VALUES (?, ?, ?) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(source)
            .bind(destination)
            .bind(edge.edge_type_id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(created)
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

    async fn list_nodes(&self, filter: &NodeFilter) -> Result<Vec<Node>, RepositoryError> {
        let scope = scope_clause(filter.scope);
        let rows = if filter.reach.within.is_some() {
            let sql = format!(
                "{REACH_CTE}SELECT {NODE_COLUMNS} FROM node n \
                 WHERE n.id IN (SELECT id FROM reach) \
                   AND (? IS NULL OR n.kind_id = (SELECT id FROM node_kind WHERE name = ?)){scope} \
                 ORDER BY n.id"
            );
            sqlx::query_as::<_, Node>(&sql)
                .bind(filter.reach.within)
                .bind(filter.reach.via.clone())
                .bind(filter.reach.via.clone())
                .bind(filter.reach.within)
                .bind(filter.reach.via.clone())
                .bind(filter.reach.via.clone())
                .bind(filter.reach.descend.clone())
                .bind(filter.kind.clone())
                .bind(filter.kind.clone())
                .fetch_all(&self.pool)
                .await?
        } else {
            let sql = format!(
                "SELECT {NODE_COLUMNS} FROM node n \
                 WHERE (? IS NULL OR n.kind_id = (SELECT id FROM node_kind WHERE name = ?)){scope} \
                 ORDER BY n.id"
            );
            sqlx::query_as::<_, Node>(&sql)
                .bind(filter.kind.clone())
                .bind(filter.kind.clone())
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows)
    }

    async fn node_kinds_for_ids(&self, ids: &[i64]) -> Result<Vec<(i64, String)>, RepositoryError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder = sqlx::QueryBuilder::new(
            "SELECT n.id, k.name FROM node n JOIN node_kind k ON k.id = n.kind_id \
             WHERE n.id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for id in ids {
                separated.push_bind(*id);
            }
        }
        builder.push(")");
        let rows = builder
            .build_query_as::<(i64, String)>()
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

    async fn neighbors(
        &self,
        node_id: i64,
        direction: NeighborDirection,
        edge_type: Option<&str>,
    ) -> Result<Vec<Neighbor>, RepositoryError> {
        let mut neighbors = Vec::new();
        if matches!(
            direction,
            NeighborDirection::Outgoing | NeighborDirection::Both
        ) {
            let rows = sqlx::query(OUTGOING_NEIGHBORS)
                .bind(node_id)
                .bind(edge_type)
                .bind(edge_type)
                .fetch_all(&self.pool)
                .await?;
            for row in &rows {
                neighbors.push(Neighbor {
                    direction: RelationDirection::Outgoing,
                    edge: Edge::from_row(row)?,
                    node: Node::from_row(row)?,
                });
            }
        }
        if matches!(
            direction,
            NeighborDirection::Incoming | NeighborDirection::Both
        ) {
            let rows = sqlx::query(INCOMING_NEIGHBORS)
                .bind(node_id)
                .bind(edge_type)
                .bind(edge_type)
                .fetch_all(&self.pool)
                .await?;
            for row in &rows {
                neighbors.push(Neighbor {
                    direction: RelationDirection::Ingoing,
                    edge: Edge::from_row(row)?,
                    node: Node::from_row(row)?,
                });
            }
        }
        Ok(neighbors)
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

    async fn search_descriptions(
        &self,
        fts_query: &str,
        reach: &ReachFilter,
    ) -> Result<Vec<(Node, bool)>, RepositoryError> {
        let select = "SELECT n.id, n.kind_id, n.name, n.description, n.content, n.updated_at, \
             EXISTS(SELECT 1 FROM edge e JOIN edge_type t ON t.id = e.edge_type_id \
                    WHERE e.destination = n.id AND t.resolves = 1) AS resolved \
             FROM node_fts JOIN node n ON n.id = node_fts.rowid";
        let rows = if reach.within.is_some() {
            let sql = format!(
                "{REACH_CTE}{select} WHERE node_fts MATCH ? \
                   AND n.id IN (SELECT id FROM reach) \
                 ORDER BY bm25(node_fts), n.id"
            );
            sqlx::query(&sql)
                .bind(reach.within)
                .bind(reach.via.clone())
                .bind(reach.via.clone())
                .bind(reach.within)
                .bind(reach.via.clone())
                .bind(reach.via.clone())
                .bind(reach.descend.clone())
                .bind(fts_query)
                .fetch_all(&self.pool)
                .await?
        } else {
            let sql = format!("{select} WHERE node_fts MATCH ? ORDER BY bm25(node_fts), n.id");
            sqlx::query(&sql)
                .bind(fts_query)
                .fetch_all(&self.pool)
                .await?
        };
        rows.iter()
            .map(|row| Ok((Node::from_row(row)?, row.try_get::<bool, _>("resolved")?)))
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
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

    async fn kind_usage(&self) -> Result<Vec<KindUsage>, RepositoryError> {
        let rows = sqlx::query_as::<_, KindUsage>(
            "SELECT k.id AS kind_id, k.name AS kind, count(n.id) AS node_count \
             FROM node_kind k LEFT JOIN node n ON n.kind_id = k.id \
             GROUP BY k.id, k.name ORDER BY k.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn edge_type_usage(&self) -> Result<Vec<EdgeTypeUsage>, RepositoryError> {
        let rows = sqlx::query_as::<_, EdgeTypeUsage>(
            "SELECT t.id AS edge_type_id, t.name AS edge_type, count(e.source) AS edge_count \
             FROM edge_type t LEFT JOIN edge e ON e.edge_type_id = t.id \
             GROUP BY t.id, t.name ORDER BY t.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn duplicate_node_rows(
        &self,
    ) -> Result<Vec<(i64, String, String, i64)>, RepositoryError> {
        let rows = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT n.kind_id, k.name, n.name, n.id \
             FROM node n \
             JOIN node_kind k ON k.id = n.kind_id \
             JOIN (SELECT kind_id, name FROM node \
                   GROUP BY kind_id, name HAVING count(*) > 1) d \
               ON d.kind_id = n.kind_id AND d.name = n.name \
             ORDER BY n.kind_id, n.name, n.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn isolated_node_count(&self) -> Result<i64, RepositoryError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM node n \
             WHERE NOT EXISTS (SELECT 1 FROM edge e WHERE e.source = n.id) \
               AND NOT EXISTS (SELECT 1 FROM edge e WHERE e.destination = n.id)",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn isolated_nodes(&self, limit: i64) -> Result<Vec<IsolatedNode>, RepositoryError> {
        let rows = sqlx::query_as::<_, IsolatedNode>(
            "SELECT n.id, k.name AS kind, n.name \
             FROM node n JOIN node_kind k ON k.id = n.kind_id \
             WHERE NOT EXISTS (SELECT 1 FROM edge e WHERE e.source = n.id) \
               AND NOT EXISTS (SELECT 1 FROM edge e WHERE e.destination = n.id) \
             ORDER BY n.id LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn is_anchored(&self, node_id: i64) -> Result<bool, RepositoryError> {
        let sql = format!(
            "{ANCESTRY_CTE}SELECT EXISTS(SELECT 1 FROM ancestry a JOIN edge b ON b.source = a.id \
             WHERE b.edge_type_id = (SELECT id FROM edge_type WHERE name = 'belongs_to'))"
        );
        let anchored = sqlx::query_scalar::<_, bool>(&sql)
            .bind(node_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(anchored)
    }

    async fn outgoing_edge_targets(
        &self,
        source: i64,
        edge_type_id: i64,
    ) -> Result<Vec<i64>, RepositoryError> {
        let rows = sqlx::query_scalar::<_, i64>(
            "SELECT destination FROM edge WHERE source = ? AND edge_type_id = ?",
        )
        .bind(source)
        .bind(edge_type_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn unanchored_work_count(&self) -> Result<i64, RepositoryError> {
        let sql = format!(
            "{ANCHORED_CTE}SELECT count(*) FROM node n JOIN node_kind k ON k.id = n.kind_id \
             WHERE k.name IN ('task', 'goal') AND n.id NOT IN (SELECT id FROM anchored)"
        );
        let count = sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    async fn unanchored_work(&self, limit: i64) -> Result<Vec<IsolatedNode>, RepositoryError> {
        let sql = format!(
            "{ANCHORED_CTE}SELECT n.id, k.name AS kind, n.name FROM node n \
             JOIN node_kind k ON k.id = n.kind_id \
             WHERE k.name IN ('task', 'goal') AND n.id NOT IN (SELECT id FROM anchored) \
             ORDER BY n.id LIMIT ?"
        );
        let rows = sqlx::query_as::<_, IsolatedNode>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn record_node_accesses(
        &self,
        session_id: &str,
        node_ids: &[i64],
        action: AccessAction,
        query_term: Option<&str>,
    ) -> Result<Vec<NodeAccessRecord>, RepositoryError> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut transaction = self.pool.begin().await?;
        let mut recorded = Vec::with_capacity(node_ids.len());
        for node_id in node_ids {
            let row = sqlx::query_as::<_, NodeAccessRecord>(
                "INSERT INTO node_access (session_id, node_id, node_name, node_kind, action, query_term) \
                 SELECT ?, n.id, n.name, k.name, ?, ? \
                 FROM node n JOIN node_kind k ON k.id = n.kind_id \
                 WHERE n.id = ? \
                 RETURNING id AS access_id, node_id",
            )
            .bind(session_id)
            .bind(action.as_str())
            .bind(query_term)
            .bind(node_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if let Some(record) = row {
                recorded.push(record);
            }
        }
        transaction.commit().await?;
        Ok(recorded)
    }

    async fn set_node_access_relevance(
        &self,
        session_id: &str,
        access_id: i64,
        relevant: bool,
    ) -> Result<bool, RepositoryError> {
        let result =
            sqlx::query("UPDATE node_access SET relevant = ? WHERE id = ? AND session_id = ?")
                .bind(relevant)
                .bind(access_id)
                .bind(session_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn prune_node_access(&self, retention_days: u64) -> Result<u64, RepositoryError> {
        let modifier = format!("-{retention_days} days");
        let result = sqlx::query(
            "DELETE FROM node_access \
             WHERE accessed_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)",
        )
        .bind(modifier)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

fn resolve_node_ref(
    node_ref: NodeRef,
    ids: &[i64],
    generated: &HashSet<i64>,
) -> Result<i64, RepositoryError> {
    match node_ref {
        NodeRef::Existing(id) if generated.contains(&id) => {
            Err(RepositoryError::ForeignKeyViolation(format!(
                "reference to node {id} collides with a node created in the same request"
            )))
        }
        NodeRef::Existing(id) => Ok(id),
        NodeRef::New(index) => ids.get(index).copied().ok_or_else(|| {
            RepositoryError::SqlExecutionError(format!(
                "node reference index {index} is out of range"
            ))
        }),
    }
}

fn escape_like_pattern(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
