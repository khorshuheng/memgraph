use std::collections::{BTreeSet, HashMap, HashSet};

use async_trait::async_trait;
use sqlx::sqlite::Sqlite;
use sqlx::{FromRow, QueryBuilder, SqliteConnection, SqlitePool};

use crate::model::{
    Edge, EdgeType, EdgeTypeUsage, IsolatedNode, KindUsage, Neighbor, NeighborDirection, Node,
    NodeFilter, NodeKind, NodeRef, PackDraft, PackEdgeType, PackItem, PackNodeKind, PackReport,
    PackStatus, PendingEdge, RelationDirection, SearchHit, SearchPage, SearchScope, TagUsage,
    encode_kind_list, split_kind_list,
};

use super::error::RepositoryError;

const NODE_KIND_COLUMNS: &str = "id, version, name, description, updated_at";
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
    async fn apply_pack(&self, pack: &PackDraft) -> Result<PackReport, RepositoryError>;

    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError>;
    async fn upsert_node_with_tags(
        &self,
        node: &Node,
        tags: &[String],
    ) -> Result<Node, RepositoryError>;
    async fn insert_nodes_with_edges(
        &self,
        nodes: &[Node],
        edges: &[PendingEdge],
    ) -> Result<Vec<Node>, RepositoryError>;
    async fn get_node(&self, id: i64) -> Result<Option<Node>, RepositoryError>;
    async fn delete_node(&self, id: i64) -> Result<bool, RepositoryError>;
    async fn list_nodes(&self, filter: &NodeFilter) -> Result<Vec<Node>, RepositoryError>;
    async fn node_kinds_for_ids(&self, ids: &[i64]) -> Result<Vec<(i64, String)>, RepositoryError>;

    async fn set_node_tags(&self, node_id: i64, tags: &[String]) -> Result<(), RepositoryError>;
    async fn node_tags(&self, node_id: i64) -> Result<Vec<String>, RepositoryError>;
    async fn node_tags_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<String>>, RepositoryError>;
    async fn list_tags(&self) -> Result<Vec<TagUsage>, RepositoryError>;

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError>;
    async fn remove_edge(&self, edge: &Edge) -> Result<bool, RepositoryError>;
    async fn neighbors(
        &self,
        node_id: i64,
        direction: NeighborDirection,
        edge_type: Option<&str>,
    ) -> Result<Vec<Neighbor>, RepositoryError>;

    async fn search_nodes(
        &self,
        query: Option<&str>,
        tags: &[String],
        limit: i64,
        offset: i64,
    ) -> Result<SearchPage, RepositoryError>;

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
}

#[derive(Clone)]
pub struct SqliteGraphRepository {
    pool: SqlitePool,
}

impl SqliteGraphRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn apply_pack_once(&self, pack: &PackDraft) -> Result<PackReport, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match apply_pack_items(&mut connection, pack).await {
            Ok(report) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(report)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
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
        insert_node(&self.pool, node).await.map_err(Into::into)
    }
    async fn apply_pack(&self, pack: &PackDraft) -> Result<PackReport, RepositoryError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.apply_pack_once(pack).await {
                Ok(report) => return Ok(report),
                Err(RepositoryError::Busy(error)) => {
                    if attempt >= 3 {
                        return Err(RepositoryError::Busy(error));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50 * attempt)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn upsert_node_with_tags(
        &self,
        node: &Node,
        tags: &[String],
    ) -> Result<Node, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let row = insert_node(&mut *transaction, node).await?;
        replace_tags(&mut transaction, row.id, tags).await?;
        transaction.commit().await?;
        Ok(row)
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
        for (query, row_direction, wanted) in [
            (
                OUTGOING_NEIGHBORS,
                RelationDirection::Outgoing,
                NeighborDirection::Outgoing,
            ),
            (
                INCOMING_NEIGHBORS,
                RelationDirection::Ingoing,
                NeighborDirection::Incoming,
            ),
        ] {
            if direction != NeighborDirection::Both && direction != wanted {
                continue;
            }
            let rows = sqlx::query(query)
                .bind(node_id)
                .bind(edge_type)
                .bind(edge_type)
                .fetch_all(&self.pool)
                .await?;
            for row in &rows {
                neighbors.push(Neighbor {
                    direction: row_direction,
                    edge: Edge::from_row(row)?,
                    node: Node::from_row(row)?,
                });
            }
        }
        Ok(neighbors)
    }

    async fn set_node_tags(&self, node_id: i64, tags: &[String]) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        replace_tags(&mut transaction, node_id, tags).await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn node_tags(&self, node_id: i64) -> Result<Vec<String>, RepositoryError> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT tag FROM node_tag WHERE node_id = ? ORDER BY tag",
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn node_tags_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<String>>, RepositoryError> {
        let mut by_node: HashMap<i64, Vec<String>> = HashMap::new();
        if ids.is_empty() {
            return Ok(by_node);
        }
        let mut builder = QueryBuilder::new("SELECT node_id, tag FROM node_tag WHERE node_id IN (");
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
        builder.push(") ORDER BY node_id, tag");
        let rows = builder
            .build_query_as::<(i64, String)>()
            .fetch_all(&self.pool)
            .await?;
        for (node_id, tag) in rows {
            by_node.entry(node_id).or_default().push(tag);
        }
        Ok(by_node)
    }

    async fn list_tags(&self) -> Result<Vec<TagUsage>, RepositoryError> {
        let rows = sqlx::query_as::<_, TagUsage>(
            "SELECT tag, count(*) AS node_count FROM node_tag \
             GROUP BY tag ORDER BY node_count DESC, tag ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn search_nodes(
        &self,
        query: Option<&str>,
        tags: &[String],
        limit: i64,
        offset: i64,
    ) -> Result<SearchPage, RepositoryError> {
        let total = if let Some(fts_query) = query {
            let mut builder = QueryBuilder::new(
                "SELECT count(*) FROM node_fts JOIN node n ON n.id = node_fts.rowid \
                 WHERE node_fts MATCH ",
            );
            builder.push_bind(fts_query);
            push_tag_filter(&mut builder, tags);
            builder
                .build_query_scalar::<i64>()
                .fetch_one(&self.pool)
                .await?
        } else {
            let mut builder = QueryBuilder::new("SELECT count(*) FROM node n WHERE 1 = 1");
            push_tag_filter(&mut builder, tags);
            builder
                .build_query_scalar::<i64>()
                .fetch_one(&self.pool)
                .await?
        };

        let rows = if let Some(fts_query) = query {
            let mut builder = QueryBuilder::new(
                "SELECT n.id, n.kind_id, n.name, n.description, n.content, n.updated_at, \
                 bm25(node_fts) AS score FROM node_fts \
                 JOIN node n ON n.id = node_fts.rowid WHERE node_fts MATCH ",
            );
            builder.push_bind(fts_query);
            push_tag_filter(&mut builder, tags);
            builder.push(" ORDER BY score, n.id LIMIT ");
            builder.push_bind(limit);
            builder.push(" OFFSET ");
            builder.push_bind(offset);
            builder
                .build_query_as::<SearchRow>()
                .fetch_all(&self.pool)
                .await?
        } else {
            let mut builder = QueryBuilder::new(
                "SELECT n.id, n.kind_id, n.name, n.description, n.content, n.updated_at, \
                 0.0 AS score FROM node n WHERE 1 = 1",
            );
            push_tag_filter(&mut builder, tags);
            builder.push(" ORDER BY n.updated_at DESC, n.id DESC LIMIT ");
            builder.push_bind(limit);
            builder.push(" OFFSET ");
            builder.push_bind(offset);
            builder
                .build_query_as::<SearchRow>()
                .fetch_all(&self.pool)
                .await?
        };

        let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
        let mut tags_by_node = self.node_tags_for_ids(&ids).await?;
        let hits = rows
            .into_iter()
            .map(|row| {
                let tags = tags_by_node.remove(&row.id).unwrap_or_default();
                row.into_hit(tags)
            })
            .collect();
        Ok(SearchPage {
            hits,
            total,
            limit,
            offset,
        })
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

#[derive(FromRow)]
struct SearchRow {
    id: i64,
    kind_id: i64,
    name: String,
    description: String,
    content: String,
    updated_at: String,
    score: f64,
}

impl SearchRow {
    fn into_hit(self, tags: Vec<String>) -> SearchHit {
        SearchHit {
            node: Node {
                id: self.id,
                kind_id: self.kind_id,
                name: self.name,
                description: self.description,
                content: self.content,
                updated_at: self.updated_at,
            },
            score: self.score.abs(),
            tags,
        }
    }
}

async fn apply_pack_items(
    connection: &mut SqliteConnection,
    pack: &PackDraft,
) -> Result<PackReport, RepositoryError> {
    let mut node_kinds = Vec::with_capacity(pack.node_kinds.len());
    for kind in &pack.node_kinds {
        node_kinds.push(apply_node_kind(&mut *connection, kind).await?);
    }
    let mut edge_types = Vec::with_capacity(pack.edge_types.len());
    for edge_type in &pack.edge_types {
        edge_types.push(apply_edge_type(&mut *connection, edge_type).await?);
    }
    Ok(PackReport {
        node_kinds,
        edge_types,
    })
}

async fn apply_node_kind(
    connection: &mut SqliteConnection,
    kind: &PackNodeKind,
) -> Result<PackItem, RepositoryError> {
    let existing = sqlx::query_as::<_, NodeKind>(&format!(
        "SELECT {NODE_KIND_COLUMNS} FROM node_kind WHERE name = ?"
    ))
    .bind(&kind.name)
    .fetch_optional(&mut *connection)
    .await?;
    match existing {
        None => {
            let description = kind.description.clone().unwrap_or_default();
            let (id, version) = sqlx::query_as::<_, (i64, i64)>(
                "INSERT INTO node_kind (name, description) VALUES (?, ?) RETURNING id, version",
            )
            .bind(&kind.name)
            .bind(description)
            .fetch_one(&mut *connection)
            .await?;
            Ok(PackItem {
                id,
                name: kind.name.clone(),
                version: Some(version),
                status: PackStatus::Created,
            })
        }
        Some(row) => {
            let description = kind
                .description
                .clone()
                .unwrap_or_else(|| row.description.clone());
            if description == row.description {
                return Ok(PackItem {
                    id: row.id,
                    name: row.name,
                    version: Some(row.version),
                    status: PackStatus::Unchanged,
                });
            }
            let (id, version) = sqlx::query_as::<_, (i64, i64)>(
                "UPDATE node_kind SET description = ?, version = version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ? RETURNING id, version",
            )
            .bind(description)
            .bind(row.id)
            .fetch_one(&mut *connection)
            .await?;
            Ok(PackItem {
                id,
                name: kind.name.clone(),
                version: Some(version),
                status: PackStatus::Updated,
            })
        }
    }
}

async fn apply_edge_type(
    connection: &mut SqliteConnection,
    edge_type: &PackEdgeType,
) -> Result<PackItem, RepositoryError> {
    let existing = sqlx::query_as::<_, EdgeType>(&format!(
        "SELECT {EDGE_TYPE_COLUMNS} FROM edge_type WHERE name = ?"
    ))
    .bind(&edge_type.name)
    .fetch_optional(&mut *connection)
    .await?;
    match existing {
        None => {
            let description = edge_type.description.clone().unwrap_or_default();
            let resolves = edge_type.resolves.unwrap_or(false);
            let single_outgoing = edge_type.single_outgoing.unwrap_or(false);
            let source_kinds = edge_type.source_kinds.as_deref().and_then(encode_kind_list);
            let destination_kinds = edge_type
                .destination_kinds
                .as_deref()
                .and_then(encode_kind_list);
            let id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO edge_type (name, description, resolves, single_outgoing, \
                 source_kinds, destination_kinds) VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(&edge_type.name)
            .bind(description)
            .bind(resolves)
            .bind(single_outgoing)
            .bind(source_kinds)
            .bind(destination_kinds)
            .fetch_one(&mut *connection)
            .await?;
            Ok(PackItem {
                id,
                name: edge_type.name.clone(),
                version: None,
                status: PackStatus::Created,
            })
        }
        Some(row) => {
            let description = edge_type
                .description
                .clone()
                .unwrap_or_else(|| row.description.clone());
            let resolves = edge_type.resolves.unwrap_or(row.resolves);
            let single_outgoing = edge_type.single_outgoing.unwrap_or(row.single_outgoing);
            let source_kinds =
                merge_kind_list(&row.source_kinds, edge_type.source_kinds.as_deref());
            let destination_kinds = merge_kind_list(
                &row.destination_kinds,
                edge_type.destination_kinds.as_deref(),
            );

            let contract_changed =
                !same_kind_set(row.source_kinds.as_deref(), source_kinds.as_deref())
                    || !same_kind_set(
                        row.destination_kinds.as_deref(),
                        destination_kinds.as_deref(),
                    )
                    || row.single_outgoing != single_outgoing;
            if contract_changed {
                let mut violations = Vec::new();
                if single_outgoing && !row.single_outgoing {
                    violations.extend(single_outgoing_violations(&mut *connection, row.id).await?);
                }
                violations.extend(
                    kind_violations(
                        &mut *connection,
                        row.id,
                        source_kinds.as_deref(),
                        destination_kinds.as_deref(),
                    )
                    .await?,
                );
                if !violations.is_empty() {
                    return Err(RepositoryError::ContractViolation(format!(
                        "edge type '{}' would leave existing edges in violation: {}",
                        edge_type.name,
                        violations.join("; ")
                    )));
                }
            }

            if description == row.description
                && resolves == row.resolves
                && single_outgoing == row.single_outgoing
                && same_kind_set(row.source_kinds.as_deref(), source_kinds.as_deref())
                && same_kind_set(
                    row.destination_kinds.as_deref(),
                    destination_kinds.as_deref(),
                )
            {
                return Ok(PackItem {
                    id: row.id,
                    name: row.name,
                    version: None,
                    status: PackStatus::Unchanged,
                });
            }

            let id = sqlx::query_scalar::<_, i64>(
                "UPDATE edge_type SET description = ?, resolves = ?, single_outgoing = ?, \
                 source_kinds = ?, destination_kinds = ?, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ? RETURNING id",
            )
            .bind(description)
            .bind(resolves)
            .bind(single_outgoing)
            .bind(source_kinds)
            .bind(destination_kinds)
            .bind(row.id)
            .fetch_one(&mut *connection)
            .await?;
            Ok(PackItem {
                id,
                name: edge_type.name.clone(),
                version: None,
                status: PackStatus::Updated,
            })
        }
    }
}

fn same_kind_set(left: Option<&str>, right: Option<&str>) -> bool {
    let left: BTreeSet<&str> = split_kind_list(left).into_iter().collect();
    let right: BTreeSet<&str> = split_kind_list(right).into_iter().collect();
    left == right
}

fn merge_kind_list(existing: &Option<String>, proposed: Option<&[String]>) -> Option<String> {
    match proposed {
        None => existing.clone(),
        Some(kinds) => {
            let encoded = encode_kind_list(kinds);
            if same_kind_set(existing.as_deref(), encoded.as_deref()) {
                existing.clone()
            } else {
                encoded
            }
        }
    }
}

async fn kind_violations(
    connection: &mut SqliteConnection,
    edge_type_id: i64,
    source_kinds: Option<&str>,
    destination_kinds: Option<&str>,
) -> Result<Vec<String>, RepositoryError> {
    let sources = split_kind_list(source_kinds);
    let destinations = split_kind_list(destination_kinds);
    if sources.is_empty() && destinations.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = QueryBuilder::new(
        "SELECT e.source, e.destination, sk.name, dk.name FROM edge e \
         JOIN node sn ON sn.id = e.source JOIN node_kind sk ON sk.id = sn.kind_id \
         JOIN node dn ON dn.id = e.destination JOIN node_kind dk ON dk.id = dn.kind_id \
         WHERE e.edge_type_id = ",
    );
    builder.push_bind(edge_type_id);
    builder.push(" AND (");
    let mut has_predicate = false;
    if !sources.is_empty() {
        builder.push("sk.name NOT IN (");
        let mut separated = builder.separated(", ");
        for kind in &sources {
            separated.push_bind(kind.to_string());
        }
        builder.push(")");
        has_predicate = true;
    }
    if !destinations.is_empty() {
        if has_predicate {
            builder.push(" OR ");
        }
        builder.push("dk.name NOT IN (");
        let mut separated = builder.separated(", ");
        for kind in &destinations {
            separated.push_bind(kind.to_string());
        }
        builder.push(")");
    }
    builder.push(") ORDER BY e.source, e.destination LIMIT 20");
    let rows = builder
        .build_query_as::<(i64, i64, String, String)>()
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(source, destination, source_kind, destination_kind)| {
            format!("edge {source}->{destination} ({source_kind}->{destination_kind})")
        })
        .collect())
}

async fn single_outgoing_violations(
    connection: &mut SqliteConnection,
    edge_type_id: i64,
) -> Result<Vec<String>, RepositoryError> {
    let rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT source, count(*) FROM edge WHERE edge_type_id = ? \
         GROUP BY source HAVING count(*) > 1 ORDER BY source LIMIT 20",
    )
    .bind(edge_type_id)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(source, count)| format!("node {source} has {count} outgoing edges"))
        .collect())
}

fn push_tag_filter(builder: &mut QueryBuilder<Sqlite>, tags: &[String]) {
    if tags.is_empty() {
        return;
    }
    builder.push(" AND n.id IN (SELECT node_id FROM node_tag WHERE tag IN (");
    let mut separated = builder.separated(", ");
    for tag in tags {
        separated.push_bind(tag.clone());
    }
    builder.push(") GROUP BY node_id HAVING COUNT(DISTINCT tag) = ");
    builder.push_bind(tags.len() as i64);
    builder.push(")");
}

async fn insert_node<'e, E>(executor: E, node: &Node) -> Result<Node, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    if node.id == 0 {
        sqlx::query_as::<_, Node>(&format!(
            "INSERT INTO node (kind_id, name, description, content) VALUES (?, ?, ?, ?) \
             RETURNING {NODE_COLUMNS}"
        ))
        .bind(node.kind_id)
        .bind(&node.name)
        .bind(&node.description)
        .bind(&node.content)
        .fetch_one(executor)
        .await
    } else {
        sqlx::query_as::<_, Node>(&format!(
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
        .fetch_one(executor)
        .await
    }
}

async fn replace_tags(
    connection: &mut SqliteConnection,
    node_id: i64,
    tags: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM node_tag WHERE node_id = ?")
        .bind(node_id)
        .execute(&mut *connection)
        .await?;
    for tag in tags {
        sqlx::query("INSERT OR IGNORE INTO node_tag (node_id, tag) VALUES (?, ?)")
            .bind(node_id)
            .bind(tag)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}
