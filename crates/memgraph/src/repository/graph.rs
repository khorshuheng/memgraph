use std::collections::{BTreeSet, HashMap, HashSet};

use async_trait::async_trait;
use sqlx::sqlite::Sqlite;
use sqlx::{FromRow, QueryBuilder, Row, SqliteConnection, SqlitePool};

use crate::model::{
    Edge, EdgeCondition, EdgeFilter, EdgeRecord, EdgeType, EdgeTypeUsage, GraphExport,
    GraphExportEdge, GraphExportNode, ImportConflict, ImportDraft, ImportEdgeResult,
    ImportNodeResult, ImportRef, ImportReport, ImportStatus, IsolatedNode, KindUsage, Neighbor,
    NeighborDirection, Node, NodeFilter, NodeKind, NodeKindProperty, NodePatch, NodeRef, PackDraft,
    PackEdgeType, PackItem, PackNodeKind, PackProperty, PackReport, PackStatus, PendingEdge,
    PendingNode, PropertyType, QueryDirection, QueryRequest, ReachFilter, RelationDirection,
    SearchHit, SearchPage, SearchScope, SortDirection, TagUsage, WorkFilter, encode_kind_list,
    split_kind_list, validate_declared_properties,
};

use super::error::RepositoryError;

const NODE_KIND_COLUMNS: &str =
    "id, version, name, description, unique_name, strict_properties, updated_at";
const EDGE_TYPE_COLUMNS: &str = "id, name, description, resolves, single_outgoing, acyclic, source_kinds, destination_kinds, \
     updated_at";
const NODE_COLUMNS: &str = "id, kind_id, name, description, content, version, updated_at";
const NODE_COLUMNS_QUALIFIED: &str =
    "n.id, n.kind_id, n.name, n.description, n.content, n.version, n.updated_at";
const MAX_EXPORT_NODES: i64 = 50_000;
const MAX_EXPORT_EDGES: i64 = 200_000;
const OUTGOING_NEIGHBORS: &str = "SELECT e.source, e.destination, e.edge_type_id, e.created_at, \
    e.properties AS edge_properties, e.version AS edge_version, \
    n.id, n.kind_id, n.name, n.description, n.content, n.version, n.updated_at \
    FROM edge e JOIN node n ON n.id = e.destination \
    WHERE e.source = ? \
      AND (? IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?)) \
    ORDER BY e.edge_type_id, e.destination";
const INCOMING_NEIGHBORS: &str = "SELECT e.source, e.destination, e.edge_type_id, e.created_at, \
    e.properties AS edge_properties, e.version AS edge_version, \
    n.id, n.kind_id, n.name, n.description, n.content, n.version, n.updated_at \
    FROM edge e JOIN node n ON n.id = e.source \
    WHERE e.destination = ? \
      AND (? IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ?)) \
    ORDER BY e.edge_type_id, e.source";
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

#[derive(Debug, Clone, FromRow)]
pub struct WorkTaskRow {
    pub id: i64,
    pub name: String,
    pub updated_at: String,
    pub done: bool,
    pub blocked: bool,
    pub goal_id: Option<i64>,
    pub goal_name: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct WorkDependencyRow {
    pub task_id: i64,
    pub dependency_id: i64,
    pub dependency_name: String,
    pub done: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct WorkImplementationRow {
    pub task_id: i64,
    pub change_id: i64,
    pub change_name: String,
}

#[derive(Debug, Clone)]
pub struct WorkSnapshot {
    pub rows: Vec<WorkTaskRow>,
    pub dependencies: Vec<WorkDependencyRow>,
    pub implementations: Vec<WorkImplementationRow>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct QueryNodeRow {
    pub id: i64,
    pub kind_id: i64,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct QueryEdgeRow {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
    pub edge_type: String,
    pub created_at: String,
    pub properties: String,
}

#[derive(Debug, Clone)]
pub struct NodeQuerySnapshot {
    pub rows: Vec<QueryNodeRow>,
    pub tags: HashMap<i64, Vec<String>>,
    pub properties: HashMap<i64, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyReservation {
    Reserved,
    Replay { status: u16, body: Vec<u8> },
    Conflict,
    InProgress,
}

#[derive(Debug, Clone)]
pub struct NodeBatchSnapshot {
    pub nodes: Vec<Node>,
    pub tags: HashMap<i64, Vec<String>>,
    pub properties: HashMap<i64, String>,
}

#[async_trait]
pub trait GraphRepository: Send + Sync {
    async fn create_node_kind(&self, node_kind: &NodeKind) -> Result<NodeKind, RepositoryError>;
    async fn list_node_kinds(&self) -> Result<Vec<NodeKind>, RepositoryError>;

    async fn create_edge_type(&self, edge_type: &EdgeType) -> Result<EdgeType, RepositoryError>;
    async fn list_edge_types(&self) -> Result<Vec<EdgeType>, RepositoryError>;
    async fn apply_pack(&self, pack: &PackDraft) -> Result<PackReport, RepositoryError>;
    async fn import_graph(&self, draft: &ImportDraft) -> Result<ImportReport, RepositoryError>;
    async fn update_node(
        &self,
        node_id: i64,
        patch: &NodePatch,
        expected_version: Option<i64>,
    ) -> Result<(Node, Vec<String>), RepositoryError>;

    async fn upsert_node(&self, node: &Node) -> Result<Node, RepositoryError>;
    async fn upsert_node_with_tags(
        &self,
        node: &Node,
        tags: &[String],
        properties: Option<&serde_json::Value>,
    ) -> Result<Node, RepositoryError>;
    async fn insert_nodes_with_edges(
        &self,
        nodes: &[PendingNode],
        edges: &[PendingEdge],
    ) -> Result<Vec<Node>, RepositoryError>;
    async fn get_node(&self, id: i64) -> Result<Option<Node>, RepositoryError>;
    async fn delete_node(&self, id: i64) -> Result<bool, RepositoryError>;
    async fn list_nodes(&self, filter: &NodeFilter) -> Result<Vec<Node>, RepositoryError>;
    async fn list_nodes_page(
        &self,
        filter: &NodeFilter,
        limit: i64,
        offset: i64,
        order: &[crate::model::QueryOrder],
        since: Option<&str>,
    ) -> Result<(Vec<Node>, i64), RepositoryError>;
    async fn node_kinds_for_ids(&self, ids: &[i64]) -> Result<Vec<(i64, String)>, RepositoryError>;
    async fn node_id_by_natural_key(
        &self,
        kind_id: i64,
        name: &str,
    ) -> Result<Option<i64>, RepositoryError>;
    async fn node_ids_by_name(&self, kind_id: i64, name: &str)
    -> Result<Vec<i64>, RepositoryError>;

    async fn set_node_tags(&self, node_id: i64, tags: &[String]) -> Result<(), RepositoryError>;
    async fn node_tags(&self, node_id: i64) -> Result<Vec<String>, RepositoryError>;
    async fn node_tags_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<String>>, RepositoryError>;
    async fn node_properties(&self, node_id: i64) -> Result<String, RepositoryError>;
    async fn node_properties_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, String>, RepositoryError>;
    async fn node_kind_unique_name(&self, kind_id: i64) -> Result<bool, RepositoryError>;
    async fn node_kind_properties(
        &self,
        kind_id: i64,
    ) -> Result<Vec<NodeKindProperty>, RepositoryError>;
    async fn node_kind_properties_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<NodeKindProperty>>, RepositoryError>;
    async fn list_tags(&self) -> Result<Vec<TagUsage>, RepositoryError>;
    async fn nodes_snapshot(&self, ids: &[i64]) -> Result<NodeBatchSnapshot, RepositoryError>;
    async fn list_edges(
        &self,
        filter: &EdgeFilter,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<EdgeRecord>, i64), RepositoryError>;
    async fn graph_export(&self) -> Result<GraphExport, RepositoryError>;
    async fn query_nodes(
        &self,
        query: &QueryRequest,
        limit: i64,
        offset: i64,
    ) -> Result<(NodeQuerySnapshot, i64), RepositoryError>;
    async fn query_edges(
        &self,
        query: &QueryRequest,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<QueryEdgeRow>, i64), RepositoryError>;
    async fn idempotency_reserve(
        &self,
        key: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<IdempotencyReservation, RepositoryError>;
    async fn idempotency_complete(
        &self,
        key: &str,
        status: i64,
        body: &[u8],
    ) -> Result<(), RepositoryError>;
    async fn idempotency_abort(&self, key: &str) -> Result<(), RepositoryError>;
    async fn work_snapshot(
        &self,
        filter: &WorkFilter,
        only_ready: bool,
        include_done: bool,
        limit: i64,
        offset: i64,
    ) -> Result<WorkSnapshot, RepositoryError>;

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError>;
    async fn add_edge_with_properties(
        &self,
        edge: &Edge,
        properties: Option<&str>,
    ) -> Result<(), RepositoryError>;
    async fn set_edge_properties(
        &self,
        edge: &Edge,
        properties: &str,
        expected_version: Option<i64>,
    ) -> Result<bool, RepositoryError>;
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
        properties: &serde_json::Map<String, serde_json::Value>,
        limit: i64,
        offset: i64,
    ) -> Result<SearchPage, RepositoryError>;

    async fn kind_usage(&self) -> Result<Vec<KindUsage>, RepositoryError>;
    async fn edge_type_usage(&self) -> Result<Vec<EdgeTypeUsage>, RepositoryError>;
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

    async fn update_node_once(
        &self,
        node_id: i64,
        patch: &NodePatch,
        expected_version: Option<i64>,
    ) -> Result<(Node, Vec<String>), RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match update_node_items(&mut connection, node_id, patch, expected_version).await {
            Ok(result) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(result)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn upsert_node_with_tags_once(
        &self,
        node: &Node,
        tags: &[String],
        properties: Option<&serde_json::Value>,
    ) -> Result<Node, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match upsert_node_with_tags_items(&mut connection, node, tags, properties).await {
            Ok(row) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(row)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn import_graph_once(
        &self,
        draft: &ImportDraft,
    ) -> Result<ImportReport, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match import_graph_items(&mut connection, draft).await {
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

    async fn insert_nodes_with_edges_once(
        &self,
        nodes: &[PendingNode],
        edges: &[PendingEdge],
    ) -> Result<Vec<Node>, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match insert_nodes_with_edges_items(&mut connection, nodes, edges).await {
            Ok(created) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(created)
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
            "INSERT INTO node_kind (name, description, unique_name, strict_properties) \
             VALUES (?, ?, ?, ?) RETURNING {NODE_KIND_COLUMNS}"
        ))
        .bind(&node_kind.name)
        .bind(&node_kind.description)
        .bind(node_kind.unique_name)
        .bind(node_kind.strict_properties)
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
            "INSERT INTO edge_type (name, description, resolves, single_outgoing, acyclic, \
             source_kinds, destination_kinds) VALUES (?, ?, ?, ?, ?, ?, ?) \
             RETURNING {EDGE_TYPE_COLUMNS}"
        ))
        .bind(&edge_type.name)
        .bind(&edge_type.description)
        .bind(edge_type.resolves)
        .bind(edge_type.single_outgoing)
        .bind(edge_type.acyclic)
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

    async fn import_graph(&self, draft: &ImportDraft) -> Result<ImportReport, RepositoryError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.import_graph_once(draft).await {
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

    async fn update_node(
        &self,
        node_id: i64,
        patch: &NodePatch,
        expected_version: Option<i64>,
    ) -> Result<(Node, Vec<String>), RepositoryError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .update_node_once(node_id, patch, expected_version)
                .await
            {
                Ok(result) => return Ok(result),
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
        properties: Option<&serde_json::Value>,
    ) -> Result<Node, RepositoryError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .upsert_node_with_tags_once(node, tags, properties)
                .await
            {
                Ok(row) => return Ok(row),
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

    async fn insert_nodes_with_edges(
        &self,
        nodes: &[PendingNode],
        edges: &[PendingEdge],
    ) -> Result<Vec<Node>, RepositoryError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.insert_nodes_with_edges_once(nodes, edges).await {
                Ok(created) => return Ok(created),
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
        self.list_nodes_page(filter, -1, 0, &[], None)
            .await
            .map(|(items, _)| items)
    }

    async fn list_nodes_page(
        &self,
        filter: &NodeFilter,
        limit: i64,
        offset: i64,
        order: &[crate::model::QueryOrder],
        since: Option<&str>,
    ) -> Result<(Vec<Node>, i64), RepositoryError> {
        let mut transaction = self.pool.begin().await?;

        let mut count = QueryBuilder::new("");
        if filter.reach.within.is_some() {
            push_reach_cte(&mut count, &filter.reach);
        }
        count.push("SELECT count(*) FROM node n JOIN node_kind k ON k.id = n.kind_id WHERE 1 = 1");
        push_node_conditions(&mut count, filter, since);
        let total = count
            .build_query_scalar::<i64>()
            .fetch_one(&mut *transaction)
            .await?;

        let mut page = QueryBuilder::new("");
        if filter.reach.within.is_some() {
            push_reach_cte(&mut page, &filter.reach);
        }
        page.push(format!(
            "SELECT {NODE_COLUMNS_QUALIFIED} FROM node n JOIN node_kind k ON k.id = n.kind_id \
             WHERE 1 = 1"
        ));
        push_node_conditions(&mut page, filter, since);
        push_node_query_order(&mut page, order);
        page.push(" LIMIT ");
        page.push_bind(limit);
        page.push(" OFFSET ");
        page.push_bind(offset);
        let items = page
            .build_query_as::<Node>()
            .fetch_all(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok((items, total))
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

    async fn node_id_by_natural_key(
        &self,
        kind_id: i64,
        name: &str,
    ) -> Result<Option<i64>, RepositoryError> {
        let id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM node WHERE kind_id = ? AND name = ? LIMIT 1",
        )
        .bind(kind_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    async fn node_ids_by_name(
        &self,
        kind_id: i64,
        name: &str,
    ) -> Result<Vec<i64>, RepositoryError> {
        let ids = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM node WHERE kind_id = ? AND name = ? ORDER BY id",
        )
        .bind(kind_id)
        .bind(name)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    async fn add_edge(&self, edge: &Edge) -> Result<(), RepositoryError> {
        self.add_edge_with_properties(edge, None).await
    }

    async fn add_edge_with_properties(
        &self,
        edge: &Edge,
        properties: Option<&str>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO edge (source, destination, edge_type_id, properties) \
             VALUES (?, ?, ?, coalesce(?, '{}')) ON CONFLICT DO NOTHING",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.edge_type_id)
        .bind(properties)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_edge_properties(
        &self,
        edge: &Edge,
        properties: &str,
        expected_version: Option<i64>,
    ) -> Result<bool, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        let existing = sqlx::query_scalar::<_, i64>(
            "SELECT version FROM edge \
             WHERE source = ? AND destination = ? AND edge_type_id = ?",
        )
        .bind(edge.source)
        .bind(edge.destination)
        .bind(edge.edge_type_id)
        .fetch_optional(&mut *connection)
        .await;
        let result = match existing {
            Ok(Some(version)) => {
                if let Some(expected) = expected_version
                    && version != expected
                {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::PreconditionFailed(format!(
                        "edge is at version {version}, not {expected}"
                    )));
                }
                sqlx::query(
                    "UPDATE edge SET properties = ?, version = version + 1 \
                     WHERE source = ? AND destination = ? AND edge_type_id = ?",
                )
                .bind(properties)
                .bind(edge.source)
                .bind(edge.destination)
                .bind(edge.edge_type_id)
                .execute(&mut *connection)
                .await
                .map(|result| result.rows_affected() > 0)
                .map_err(RepositoryError::from)
            }
            Ok(None) => Ok(false),
            Err(error) => Err(RepositoryError::from(error)),
        };
        match result {
            Ok(updated) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(updated)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
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
                    edge_properties: row.try_get("edge_properties")?,
                    edge_version: row.try_get("edge_version")?,
                });
            }
        }
        Ok(neighbors)
    }

    async fn set_node_tags(&self, node_id: i64, tags: &[String]) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let current = node_tags_in(&mut transaction, node_id).await?;
        if !tags_equal(&current, tags) {
            replace_tags(&mut transaction, node_id, tags).await?;
            sqlx::query(
                "UPDATE node SET version = version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
            )
            .bind(node_id)
            .execute(&mut *transaction)
            .await?;
        }
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

    async fn node_properties(&self, node_id: i64) -> Result<String, RepositoryError> {
        let properties =
            sqlx::query_scalar::<_, String>("SELECT properties FROM node WHERE id = ?")
                .bind(node_id)
                .fetch_optional(&self.pool)
                .await?;
        properties.ok_or(RepositoryError::RowNotFound)
    }

    async fn node_properties_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, String>, RepositoryError> {
        let mut by_node: HashMap<i64, String> = HashMap::new();
        if ids.is_empty() {
            return Ok(by_node);
        }
        let mut builder = QueryBuilder::new("SELECT id, properties FROM node WHERE id IN (");
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
        builder.push(")");
        let rows = builder
            .build_query_as::<(i64, String)>()
            .fetch_all(&self.pool)
            .await?;
        for (node_id, properties) in rows {
            by_node.insert(node_id, properties);
        }
        Ok(by_node)
    }

    async fn node_kind_unique_name(&self, kind_id: i64) -> Result<bool, RepositoryError> {
        let unique =
            sqlx::query_scalar::<_, bool>("SELECT unique_name FROM node_kind WHERE id = ?")
                .bind(kind_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(unique.unwrap_or(true))
    }

    async fn node_kind_properties(
        &self,
        kind_id: i64,
    ) -> Result<Vec<NodeKindProperty>, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        load_kind_properties(&mut connection, kind_id).await
    }

    async fn node_kind_properties_for_ids(
        &self,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<NodeKindProperty>>, RepositoryError> {
        let mut by_kind: HashMap<i64, Vec<NodeKindProperty>> = HashMap::new();
        if ids.is_empty() {
            return Ok(by_kind);
        }
        let mut builder = QueryBuilder::new(
            "SELECT kind_id, key, value_type, required, indexed FROM node_kind_property \
             WHERE kind_id IN (",
        );
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
        builder.push(") ORDER BY kind_id, key");
        let rows = builder
            .build_query_as::<(i64, String, String, bool, bool)>()
            .fetch_all(&self.pool)
            .await?;
        for (kind_id, key, value_type, required, indexed) in rows {
            by_kind.entry(kind_id).or_default().push(NodeKindProperty {
                key,
                value_type: PropertyType::parse(&value_type).unwrap_or_default(),
                required,
                indexed,
            });
        }
        Ok(by_kind)
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

    async fn work_snapshot(
        &self,
        filter: &WorkFilter,
        only_ready: bool,
        include_done: bool,
        limit: i64,
        offset: i64,
    ) -> Result<WorkSnapshot, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match work_snapshot_items(
            &mut connection,
            filter,
            only_ready,
            include_done,
            limit,
            offset,
        )
        .await
        {
            Ok(snapshot) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(snapshot)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn nodes_snapshot(&self, ids: &[i64]) -> Result<NodeBatchSnapshot, RepositoryError> {
        if ids.is_empty() {
            return Ok(NodeBatchSnapshot {
                nodes: Vec::new(),
                tags: HashMap::new(),
                properties: HashMap::new(),
            });
        }
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        let result = node_batch_items(&mut connection, ids).await;
        match result {
            Ok(snapshot) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(snapshot)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn list_edges(
        &self,
        filter: &EdgeFilter,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<EdgeRecord>, i64), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let mut count = QueryBuilder::new("SELECT count(*) FROM edge e WHERE 1 = 1");
        push_edge_conditions(&mut count, filter);
        let total = count
            .build_query_scalar::<i64>()
            .fetch_one(&mut *transaction)
            .await?;
        let mut page = QueryBuilder::new(
            "SELECT e.source, e.destination, e.edge_type_id, e.version, e.created_at, \
               e.properties AS properties FROM edge e WHERE 1 = 1",
        );
        push_edge_conditions(&mut page, filter);
        page.push(" ORDER BY e.source, e.edge_type_id, e.destination LIMIT ");
        page.push_bind(limit);
        page.push(" OFFSET ");
        page.push_bind(offset);
        let items = page
            .build_query_as::<EdgeRecord>()
            .fetch_all(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok((items, total))
    }

    async fn graph_export(&self) -> Result<GraphExport, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        let node_count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM node")
            .fetch_one(&mut *connection)
            .await?;
        let edge_count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edge")
            .fetch_one(&mut *connection)
            .await?;
        if node_count > MAX_EXPORT_NODES || edge_count > MAX_EXPORT_EDGES {
            let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
            return Err(RepositoryError::LimitExceeded(format!(
                "graph has {node_count} nodes and {edge_count} edges; export is limited to \
                 {MAX_EXPORT_NODES} nodes and {MAX_EXPORT_EDGES} edges"
            )));
        }
        match graph_export_items(&mut connection).await {
            Ok(export) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(export)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn query_nodes(
        &self,
        query: &QueryRequest,
        limit: i64,
        offset: i64,
    ) -> Result<(NodeQuerySnapshot, i64), RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match query_nodes_items(&mut connection, query, limit, offset).await {
            Ok(result) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(result)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn query_edges(
        &self,
        query: &QueryRequest,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<QueryEdgeRow>, i64), RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        match query_edges_items(&mut connection, query, limit, offset).await {
            Ok(result) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(result)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn idempotency_reserve(
        &self,
        key: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<IdempotencyReservation, RepositoryError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .map_err(RepositoryError::from)?;
        let result = idempotency_reserve_items(&mut connection, key, method, path, body).await;
        match result {
            Ok(reservation) => {
                if let Err(error) = sqlx::query("COMMIT").execute(&mut *connection).await {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                    return Err(RepositoryError::from(error));
                }
                Ok(reservation)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    async fn idempotency_complete(
        &self,
        key: &str,
        status: i64,
        body: &[u8],
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "UPDATE idempotency_key SET status = ?, response_body = ? \
             WHERE key = ? AND status IS NULL",
        )
        .bind(status)
        .bind(body.to_vec())
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn idempotency_abort(&self, key: &str) -> Result<(), RepositoryError> {
        sqlx::query("DELETE FROM idempotency_key WHERE key = ? AND status IS NULL")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn search_nodes(
        &self,
        query: Option<&str>,
        tags: &[String],
        properties: &serde_json::Map<String, serde_json::Value>,
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
            push_property_filters(&mut builder, properties);
            builder
                .build_query_scalar::<i64>()
                .fetch_one(&self.pool)
                .await?
        } else {
            let mut builder = QueryBuilder::new("SELECT count(*) FROM node n WHERE 1 = 1");
            push_tag_filter(&mut builder, tags);
            push_property_filters(&mut builder, properties);
            builder
                .build_query_scalar::<i64>()
                .fetch_one(&self.pool)
                .await?
        };

        let rows = if let Some(fts_query) = query {
            let mut builder = QueryBuilder::new(
                "SELECT n.id, n.kind_id, n.name, n.description, n.content, n.version, \
                 n.updated_at, bm25(node_fts) AS score FROM node_fts \
                 JOIN node n ON n.id = node_fts.rowid WHERE node_fts MATCH ",
            );
            builder.push_bind(fts_query);
            push_tag_filter(&mut builder, tags);
            push_property_filters(&mut builder, properties);
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
                "SELECT n.id, n.kind_id, n.name, n.description, n.content, n.version, \
                 n.updated_at, 0.0 AS score FROM node n WHERE 1 = 1",
            );
            push_tag_filter(&mut builder, tags);
            push_property_filters(&mut builder, properties);
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
    version: i64,
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
                version: self.version,
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
    let declared = kind
        .properties
        .as_ref()
        .map(|properties| declared_properties(properties));
    let existing = sqlx::query_as::<_, NodeKind>(&format!(
        "SELECT {NODE_KIND_COLUMNS} FROM node_kind WHERE name = ?"
    ))
    .bind(&kind.name)
    .fetch_optional(&mut *connection)
    .await?;
    match existing {
        None => {
            let description = kind.description.clone().unwrap_or_default();
            let unique_name = kind.unique_name.unwrap_or(true);
            let strict_properties = kind.strict_properties.unwrap_or(false);
            let (id, version) = sqlx::query_as::<_, (i64, i64)>(
                "INSERT INTO node_kind (name, description, unique_name, strict_properties) \
                 VALUES (?, ?, ?, ?) RETURNING id, version",
            )
            .bind(&kind.name)
            .bind(description)
            .bind(unique_name)
            .bind(strict_properties)
            .fetch_one(&mut *connection)
            .await?;
            if let Some(declared) = &declared {
                replace_kind_properties(&mut *connection, id, declared).await?;
            }
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
            let description_changed = description != row.description;
            let unique_name = kind.unique_name.unwrap_or(row.unique_name);
            let unique_changed = unique_name != row.unique_name;
            let strict_properties = kind.strict_properties.unwrap_or(row.strict_properties);
            let strict_changed = strict_properties != row.strict_properties;
            if unique_changed && unique_name {
                let duplicates = duplicate_kind_names(&mut *connection, row.id).await?;
                if !duplicates.is_empty() {
                    let names = duplicates
                        .iter()
                        .map(|(name, count)| format!("'{name}' x{count}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(RepositoryError::ContractViolation(format!(
                        "node kind '{}' cannot require unique names while duplicates exist: {names}",
                        kind.name
                    )));
                }
            }
            let declarations_changed = match &declared {
                None => false,
                Some(declared) => {
                    load_kind_properties(&mut *connection, row.id).await? != *declared
                }
            };
            if !description_changed && !unique_changed && !strict_changed && !declarations_changed {
                return Ok(PackItem {
                    id: row.id,
                    name: row.name,
                    version: Some(row.version),
                    status: PackStatus::Unchanged,
                });
            }
            if declarations_changed || strict_changed {
                let effective = match &declared {
                    Some(declared) => declared.clone(),
                    None => load_kind_properties(&mut *connection, row.id).await?,
                };
                validate_kind_nodes(&mut *connection, row.id, &effective, strict_properties)
                    .await?;
            }
            if declarations_changed {
                replace_kind_properties(
                    &mut *connection,
                    row.id,
                    declared.as_deref().unwrap_or_default(),
                )
                .await?;
            }
            let (id, version) = sqlx::query_as::<_, (i64, i64)>(
                "UPDATE node_kind SET description = ?, unique_name = ?, \
                 strict_properties = ?, version = version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ? RETURNING id, version",
            )
            .bind(description)
            .bind(unique_name)
            .bind(strict_properties)
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

fn declared_properties(properties: &[PackProperty]) -> Vec<NodeKindProperty> {
    let mut declared: Vec<NodeKindProperty> = properties
        .iter()
        .map(|property| NodeKindProperty {
            key: property.key.clone(),
            value_type: PropertyType::parse(property.value_type.as_deref().unwrap_or("any"))
                .unwrap_or_default(),
            required: property.required,
            indexed: property.indexed,
        })
        .collect();
    declared.sort_by(|left, right| left.key.cmp(&right.key));
    declared
}

struct KindSchema {
    properties: Vec<NodeKindProperty>,
    strict: bool,
}

async fn validate_kind_nodes(
    connection: &mut SqliteConnection,
    kind_id: i64,
    declarations: &[NodeKindProperty],
    strict: bool,
) -> Result<(), RepositoryError> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, properties FROM node WHERE kind_id = ? ORDER BY id",
    )
    .bind(kind_id)
    .fetch_all(&mut *connection)
    .await?;
    let mut failures: Vec<String> = Vec::new();
    for (id, raw) in rows {
        let value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => {
                failures.push(format!("node {id}: invalid properties JSON"));
                if failures.len() >= 10 {
                    break;
                }
                continue;
            }
        };
        if let Err(error) = validate_declared_properties(&value, declarations, strict) {
            failures.push(format!("node {id}: {error}"));
            if failures.len() >= 10 {
                break;
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(RepositoryError::ContractViolation(format!(
            "node kind cannot tighten its declarations while existing nodes violate them: {}",
            failures.join("; ")
        )))
    }
}

async fn load_kind_schema(
    connection: &mut SqliteConnection,
    kind_id: i64,
) -> Result<KindSchema, RepositoryError> {
    let properties = load_kind_properties(&mut *connection, kind_id).await?;
    let strict =
        sqlx::query_scalar::<_, bool>("SELECT strict_properties FROM node_kind WHERE id = ?")
            .bind(kind_id)
            .fetch_optional(&mut *connection)
            .await?
            .unwrap_or(false);
    Ok(KindSchema { properties, strict })
}

async fn duplicate_kind_names(
    connection: &mut SqliteConnection,
    kind_id: i64,
) -> Result<Vec<(String, i64)>, RepositoryError> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT name, count(*) FROM node WHERE kind_id = ? \
         GROUP BY name HAVING count(*) > 1 ORDER BY name LIMIT 10",
    )
    .bind(kind_id)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows)
}

async fn load_kind_properties(
    connection: &mut SqliteConnection,
    kind_id: i64,
) -> Result<Vec<NodeKindProperty>, RepositoryError> {
    let rows = sqlx::query_as::<_, (String, String, bool, bool)>(
        "SELECT key, value_type, required, indexed FROM node_kind_property \
         WHERE kind_id = ? ORDER BY key",
    )
    .bind(kind_id)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(key, value_type, required, indexed)| NodeKindProperty {
            key,
            value_type: PropertyType::parse(&value_type).unwrap_or_default(),
            required,
            indexed,
        })
        .collect())
}

async fn replace_kind_properties(
    connection: &mut SqliteConnection,
    kind_id: i64,
    properties: &[NodeKindProperty],
) -> Result<(), RepositoryError> {
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name GLOB ?",
    )
    .bind(format!("idx_node_prop_{kind_id}_*"))
    .fetch_all(&mut *connection)
    .await?;
    for name in existing {
        sqlx::query(&format!("DROP INDEX IF EXISTS \"{name}\""))
            .execute(&mut *connection)
            .await?;
    }

    sqlx::query("DELETE FROM node_kind_property WHERE kind_id = ?")
        .bind(kind_id)
        .execute(&mut *connection)
        .await?;
    for property in properties {
        sqlx::query(
            "INSERT INTO node_kind_property (kind_id, key, value_type, required, indexed) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(kind_id)
        .bind(&property.key)
        .bind(property.value_type.as_str())
        .bind(property.required)
        .bind(property.indexed)
        .execute(&mut *connection)
        .await?;
    }
    for property in properties.iter().filter(|property| property.indexed) {
        let name = property_index_name(kind_id, &property.key);
        let path = json_path_literal(&property.key);
        sqlx::query(&format!(
            "CREATE INDEX IF NOT EXISTS \"{name}\" \
             ON node(json_extract(properties, '{path}'))"
        ))
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

fn property_index_name(kind_id: i64, key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("idx_node_prop_{kind_id}_{hash:016x}")
}

fn json_path_literal(key: &str) -> String {
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("$.\"{escaped}\"").replace('\'', "''")
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
            let acyclic = edge_type.acyclic.unwrap_or(false);
            let source_kinds = edge_type.source_kinds.as_deref().and_then(encode_kind_list);
            let destination_kinds = edge_type
                .destination_kinds
                .as_deref()
                .and_then(encode_kind_list);
            let id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO edge_type (name, description, resolves, single_outgoing, acyclic, \
                 source_kinds, destination_kinds) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(&edge_type.name)
            .bind(description)
            .bind(resolves)
            .bind(single_outgoing)
            .bind(acyclic)
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
            let acyclic = edge_type.acyclic.unwrap_or(row.acyclic);
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
                    || row.single_outgoing != single_outgoing
                    || row.acyclic != acyclic;
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
                && acyclic == row.acyclic
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
                 acyclic = ?, source_kinds = ?, destination_kinds = ?, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ? RETURNING id",
            )
            .bind(description)
            .bind(resolves)
            .bind(single_outgoing)
            .bind(acyclic)
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

async fn import_graph_items(
    connection: &mut SqliteConnection,
    draft: &ImportDraft,
) -> Result<ImportReport, RepositoryError> {
    let kinds = load_node_kind_ids(connection).await?;
    let edge_types = load_edge_types(connection).await?;
    let mut resolved: HashMap<String, i64> = HashMap::new();
    let mut nodes = Vec::with_capacity(draft.nodes.len());
    for node in &draft.nodes {
        let kind_id = *kinds.get(&node.kind).ok_or_else(|| {
            RepositoryError::ContractViolation(format!("node kind '{}' is not defined", node.kind))
        })?;
        let result = import_node(connection, draft.on_conflict, kind_id, node).await?;
        if let Some(reference) = &node.reference {
            resolved.insert(reference.clone(), result.id);
        }
        nodes.push(result);
    }

    let mut edges = Vec::with_capacity(draft.edges.len());
    for edge in &draft.edges {
        let source = resolve_import_ref(connection, &edge.source, &resolved).await?;
        let destination = resolve_import_ref(connection, &edge.destination, &resolved).await?;
        let edge_type = edge_types.get(&edge.edge_type).ok_or_else(|| {
            RepositoryError::ContractViolation(format!(
                "edge type '{}' is not defined",
                edge.edge_type
            ))
        })?;
        validate_import_edge(connection, edge_type, source, destination).await?;
        let encoded = edge
            .properties
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| RepositoryError::SqlExecutionError(error.to_string()))?;
        let result = sqlx::query(
            "INSERT INTO edge (source, destination, edge_type_id, properties) \
             VALUES (?, ?, ?, coalesce(?, '{}')) ON CONFLICT DO NOTHING",
        )
        .bind(source)
        .bind(destination)
        .bind(edge_type.id)
        .bind(encoded.clone())
        .execute(&mut *connection)
        .await?;
        let status = if result.rows_affected() == 1 {
            ImportStatus::Created
        } else {
            if let Some(encoded) = &encoded {
                sqlx::query(
                    "UPDATE edge SET properties = ? \
                     WHERE source = ? AND destination = ? AND edge_type_id = ?",
                )
                .bind(encoded)
                .bind(source)
                .bind(destination)
                .bind(edge_type.id)
                .execute(&mut *connection)
                .await?;
            }
            ImportStatus::Existing
        };
        edges.push(ImportEdgeResult {
            source,
            destination,
            edge_type: edge.edge_type.clone(),
            status,
        });
    }

    Ok(ImportReport { nodes, edges })
}

async fn load_node_kind_ids(
    connection: &mut SqliteConnection,
) -> Result<HashMap<String, i64>, RepositoryError> {
    let rows = sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM node_kind")
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows.into_iter().map(|(id, name)| (name, id)).collect())
}

async fn load_edge_types(
    connection: &mut SqliteConnection,
) -> Result<HashMap<String, EdgeType>, RepositoryError> {
    let rows = sqlx::query_as::<_, EdgeType>(&format!("SELECT {EDGE_TYPE_COLUMNS} FROM edge_type"))
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|edge_type| (edge_type.name.clone(), edge_type))
        .collect())
}

async fn import_node(
    connection: &mut SqliteConnection,
    conflict: ImportConflict,
    kind_id: i64,
    node: &crate::model::ImportNode,
) -> Result<ImportNodeResult, RepositoryError> {
    let schema = load_kind_schema(&mut *connection, kind_id).await?;
    let existing = match node.id {
        Some(id) => {
            sqlx::query_as::<_, (i64, i64, String, String, String, String)>(
                "SELECT id, kind_id, name, description, content, properties \
                 FROM node WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?
        }
        None => {
            let matches = sqlx::query_as::<_, (i64, i64, String, String, String, String)>(
                "SELECT id, kind_id, name, description, content, properties \
                 FROM node WHERE kind_id = ? AND name = ? ORDER BY id",
            )
            .bind(kind_id)
            .bind(&node.name)
            .fetch_all(&mut *connection)
            .await?;
            if matches.len() > 1 {
                return Err(RepositoryError::ContractViolation(format!(
                    "node name '{}' is not unique for its kind; import cannot choose between {} nodes",
                    node.name,
                    matches.len()
                )));
            }
            matches.into_iter().next()
        }
    };

    match existing {
        None => {
            let candidate = node
                .properties
                .clone()
                .unwrap_or_else(|| serde_json::json!({}));
            validate_declared_properties(&candidate, &schema.properties, schema.strict)
                .map_err(RepositoryError::ContractViolation)?;
            let properties = match &node.properties {
                Some(value) => serde_json::to_string(value)
                    .map_err(|error| RepositoryError::SqlExecutionError(error.to_string()))?,
                None => "{}".to_string(),
            };
            let id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO node (id, kind_id, name, description, content, properties) \
                 VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(node.id)
            .bind(kind_id)
            .bind(&node.name)
            .bind(node.description.clone().unwrap_or_default())
            .bind(node.content.clone().unwrap_or_default())
            .bind(properties)
            .fetch_one(&mut *connection)
            .await?;
            if let Some(tags) = &node.tags {
                replace_tags(&mut *connection, id, tags).await?;
            }
            Ok(ImportNodeResult {
                reference: node.reference.clone(),
                id,
                status: ImportStatus::Created,
            })
        }
        Some((id, existing_kind, existing_name, description, content, properties)) => {
            if conflict == ImportConflict::Skip {
                return Ok(ImportNodeResult {
                    reference: node.reference.clone(),
                    id,
                    status: ImportStatus::Skipped,
                });
            }
            if existing_kind != kind_id {
                let from = kind_name_by_id(&mut *connection, existing_kind).await?;
                let to = kind_name_by_id(&mut *connection, kind_id).await?;
                return Err(RepositoryError::ContractViolation(format!(
                    "node {id} cannot change kind from '{from}' to '{to}'"
                )));
            }
            let name_changed = existing_name != node.name;
            if let Some(value) = &node.properties {
                validate_declared_properties(value, &schema.properties, schema.strict)
                    .map_err(RepositoryError::ContractViolation)?;
            }
            let next_description = node.description.clone().unwrap_or(description.clone());
            let next_content = node.content.clone().unwrap_or(content.clone());
            let tags_changed = match &node.tags {
                None => false,
                Some(tags) => !tags_equal(&node_tags_in(&mut *connection, id).await?, tags),
            };
            let (next_properties, properties_changed) = match &node.properties {
                Some(value) => {
                    let encoded = serde_json::to_string(value)
                        .map_err(|error| RepositoryError::SqlExecutionError(error.to_string()))?;
                    let changed = encoded != properties;
                    (encoded, changed)
                }
                None => (properties.clone(), false),
            };
            if !name_changed
                && next_description == description
                && next_content == content
                && !tags_changed
                && !properties_changed
            {
                return Ok(ImportNodeResult {
                    reference: node.reference.clone(),
                    id,
                    status: ImportStatus::Unchanged,
                });
            }
            sqlx::query(
                "UPDATE node SET name = ?, description = ?, content = ?, \
                 properties = ?, version = version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
            )
            .bind(&node.name)
            .bind(next_description)
            .bind(next_content)
            .bind(next_properties)
            .bind(id)
            .execute(&mut *connection)
            .await?;
            if tags_changed {
                replace_tags(
                    &mut *connection,
                    id,
                    node.tags.as_deref().unwrap_or_default(),
                )
                .await?;
            }
            Ok(ImportNodeResult {
                reference: node.reference.clone(),
                id,
                status: ImportStatus::Updated,
            })
        }
    }
}

async fn resolve_import_ref(
    connection: &mut SqliteConnection,
    reference: &ImportRef,
    resolved: &HashMap<String, i64>,
) -> Result<i64, RepositoryError> {
    match reference {
        ImportRef::Id(id) => {
            let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM node WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *connection)
                .await?;
            exists.map(|_| *id).ok_or_else(|| {
                RepositoryError::ContractViolation(format!("node {id} does not exist"))
            })
        }
        ImportRef::Ref(reference) => resolved.get(reference).copied().ok_or_else(|| {
            RepositoryError::ContractViolation(format!(
                "ref '{reference}' is not defined in this import"
            ))
        }),
        ImportRef::NaturalKey { kind, name } => {
            let rows = sqlx::query_scalar::<_, i64>(
                "SELECT n.id FROM node n JOIN node_kind k ON k.id = n.kind_id \
                 WHERE k.name = ? AND n.name = ? ORDER BY n.id LIMIT 2",
            )
            .bind(kind)
            .bind(name)
            .fetch_all(&mut *connection)
            .await?;
            match rows.as_slice() {
                [] => Err(RepositoryError::ContractViolation(format!(
                    "no node with kind '{kind}' and name '{name}'"
                ))),
                [id] => Ok(*id),
                _ => Err(RepositoryError::ContractViolation(format!(
                    "natural key for kind '{kind}' and name '{name}' is not unique; \
                     import cannot choose between nodes"
                ))),
            }
        }
    }
}

async fn validate_import_edge(
    connection: &mut SqliteConnection,
    edge_type: &EdgeType,
    source: i64,
    destination: i64,
) -> Result<(), RepositoryError> {
    let source_kind = node_kind_name(connection, source).await?;
    let destination_kind = node_kind_name(connection, destination).await?;
    let sources = edge_type.source_kind_list();
    if !sources.is_empty() && !sources.contains(&source_kind.as_str()) {
        return Err(RepositoryError::ContractViolation(format!(
            "edge type '{}' does not accept a '{source_kind}' source",
            edge_type.name
        )));
    }
    let destinations = edge_type.destination_kind_list();
    if !destinations.is_empty() && !destinations.contains(&destination_kind.as_str()) {
        return Err(RepositoryError::ContractViolation(format!(
            "edge type '{}' does not accept a '{destination_kind}' destination",
            edge_type.name
        )));
    }
    if edge_type.single_outgoing {
        let targets = sqlx::query_scalar::<_, i64>(
            "SELECT destination FROM edge WHERE source = ? AND edge_type_id = ?",
        )
        .bind(source)
        .bind(edge_type.id)
        .fetch_all(&mut *connection)
        .await?;
        if targets.iter().any(|target| *target != destination) {
            return Err(RepositoryError::ContractViolation(format!(
                "edge type '{}' allows one outgoing edge, and node {source} already points at {}",
                edge_type.name,
                targets
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
    Ok(())
}

async fn kind_name_by_id(
    connection: &mut SqliteConnection,
    kind_id: i64,
) -> Result<String, RepositoryError> {
    let name = sqlx::query_scalar::<_, String>("SELECT name FROM node_kind WHERE id = ?")
        .bind(kind_id)
        .fetch_optional(&mut *connection)
        .await?;
    name.ok_or_else(|| {
        RepositoryError::ContractViolation(format!("node kind {kind_id} is not defined"))
    })
}

async fn node_kind_name(
    connection: &mut SqliteConnection,
    node_id: i64,
) -> Result<String, RepositoryError> {
    let name = sqlx::query_scalar::<_, String>(
        "SELECT k.name FROM node n JOIN node_kind k ON k.id = n.kind_id WHERE n.id = ?",
    )
    .bind(node_id)
    .fetch_optional(&mut *connection)
    .await?;
    name.ok_or_else(|| RepositoryError::ContractViolation(format!("node {node_id} does not exist")))
}

async fn update_node_items(
    connection: &mut SqliteConnection,
    node_id: i64,
    patch: &NodePatch,
    expected_version: Option<i64>,
) -> Result<(Node, Vec<String>), RepositoryError> {
    let existing =
        sqlx::query_as::<_, Node>(&format!("SELECT {NODE_COLUMNS} FROM node WHERE id = ?"))
            .bind(node_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(RepositoryError::RowNotFound)?;

    if let Some(expected) = expected_version
        && existing.version != expected
    {
        return Err(RepositoryError::PreconditionFailed(format!(
            "node {node_id} is at version {}, not {expected}",
            existing.version
        )));
    }

    if let Some(properties) = &patch.properties {
        let schema = load_kind_schema(&mut *connection, existing.kind_id).await?;
        validate_declared_properties(properties, &schema.properties, schema.strict)
            .map_err(RepositoryError::ContractViolation)?;
    }

    let name = patch.name.clone().unwrap_or_else(|| existing.name.clone());
    let description = patch
        .description
        .clone()
        .unwrap_or_else(|| existing.description.clone());
    let content = patch
        .content
        .clone()
        .unwrap_or_else(|| existing.content.clone());

    let name_changed = name != existing.name;
    if name_changed {
        let unique_name =
            sqlx::query_scalar::<_, bool>("SELECT unique_name FROM node_kind WHERE id = ?")
                .bind(existing.kind_id)
                .fetch_optional(&mut *connection)
                .await?
                .unwrap_or(true);
        let conflict = if unique_name {
            sqlx::query_scalar::<_, i64>(
                "SELECT id FROM node WHERE kind_id = ? AND name = ? LIMIT 1",
            )
            .bind(existing.kind_id)
            .bind(&name)
            .fetch_optional(&mut *connection)
            .await?
        } else {
            None
        };
        if let Some(other) = conflict
            && other != node_id
        {
            return Err(RepositoryError::UniqueConstraintViolation(format!(
                "a node named '{name}' already exists with id {other}"
            )));
        }
    }

    let current_properties =
        sqlx::query_scalar::<_, String>("SELECT properties FROM node WHERE id = ?")
            .bind(node_id)
            .fetch_one(&mut *connection)
            .await?;
    let (properties, properties_changed) = match &patch.properties {
        Some(value) => {
            let encoded = serde_json::to_string(value)
                .map_err(|error| RepositoryError::SqlExecutionError(error.to_string()))?;
            let changed = encoded != current_properties;
            (encoded, changed)
        }
        None => (current_properties, false),
    };

    let current_tags = node_tags_in(&mut *connection, node_id).await?;
    let (tags, tags_changed) = match &patch.tags {
        Some(proposed) => {
            let changed = !tags_equal(&current_tags, proposed);
            if changed {
                replace_tags(&mut *connection, node_id, proposed).await?;
            }
            (proposed.clone(), changed)
        }
        None => (current_tags, false),
    };

    let fields_changed = name_changed
        || description != existing.description
        || content != existing.content
        || tags_changed
        || properties_changed;
    let node = if fields_changed {
        sqlx::query_as::<_, Node>(&format!(
            "UPDATE node SET name = ?, description = ?, content = ?, properties = ?, \
             version = version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ? RETURNING {NODE_COLUMNS}"
        ))
        .bind(name)
        .bind(description)
        .bind(content)
        .bind(properties)
        .bind(node_id)
        .fetch_one(&mut *connection)
        .await?
    } else {
        existing
    };

    Ok((node, tags))
}

async fn insert_nodes_with_edges_items(
    connection: &mut SqliteConnection,
    nodes: &[PendingNode],
    edges: &[PendingEdge],
) -> Result<Vec<Node>, RepositoryError> {
    let mut schemas: HashMap<i64, KindSchema> = HashMap::new();
    let mut created = Vec::with_capacity(nodes.len());
    let mut ids = Vec::with_capacity(nodes.len());
    for pending in nodes {
        let node = &pending.node;
        if node.id != 0 {
            let existing_kind =
                sqlx::query_scalar::<_, i64>("SELECT kind_id FROM node WHERE id = ?")
                    .bind(node.id)
                    .fetch_optional(&mut *connection)
                    .await?;
            if let Some(existing_kind) = existing_kind
                && existing_kind != node.kind_id
            {
                let from = kind_name_by_id(&mut *connection, existing_kind).await?;
                let to = kind_name_by_id(&mut *connection, node.kind_id).await?;
                return Err(RepositoryError::ContractViolation(format!(
                    "node {} cannot change kind from '{from}' to '{to}'",
                    node.id
                )));
            }
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = schemas.entry(node.kind_id) {
            let loaded = load_kind_schema(&mut *connection, node.kind_id).await?;
            entry.insert(loaded);
        }
        let empty = serde_json::json!({});
        let candidate = pending.properties.as_ref().unwrap_or(&empty);
        if pending.properties.is_some() || node.id == 0 {
            let schema = schemas.get(&node.kind_id).unwrap();
            validate_declared_properties(candidate, &schema.properties, schema.strict)
                .map_err(RepositoryError::ContractViolation)?;
        }
        let mut row = if node.id == 0 {
            sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (kind_id, name, description, content) VALUES (?, ?, ?, ?) \
                 RETURNING {NODE_COLUMNS}"
            ))
            .bind(node.kind_id)
            .bind(&node.name)
            .bind(&node.description)
            .bind(&node.content)
            .fetch_one(&mut *connection)
            .await?
        } else {
            sqlx::query_as::<_, Node>(&format!(
                "INSERT INTO node (id, kind_id, name, description, content) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET kind_id = excluded.kind_id, \
                 name = excluded.name, description = excluded.description, \
                 content = excluded.content, version = CASE WHEN node.kind_id = excluded.kind_id \
                   AND node.name = excluded.name AND node.description = excluded.description \
                   AND node.content = excluded.content THEN node.version ELSE node.version + 1 END, \
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
            .fetch_one(&mut *connection)
            .await?
        };
        let mut touched = false;
        if let Some(properties) = &pending.properties {
            let encoded = serde_json::to_string(properties)
                .map_err(|error| RepositoryError::SqlExecutionError(error.to_string()))?;
            let current =
                sqlx::query_scalar::<_, String>("SELECT properties FROM node WHERE id = ?")
                    .bind(row.id)
                    .fetch_one(&mut *connection)
                    .await?;
            if encoded != current {
                sqlx::query("UPDATE node SET properties = ? WHERE id = ?")
                    .bind(encoded)
                    .bind(row.id)
                    .execute(&mut *connection)
                    .await?;
                touched = true;
            }
        }
        if let Some(tags) = &pending.tags {
            let current = node_tags_in(&mut *connection, row.id).await?;
            if !tags_equal(&current, tags) {
                replace_tags(&mut *connection, row.id, tags).await?;
                touched = true;
            }
        }
        if touched && node.id != 0 {
            row = sqlx::query_as::<_, Node>(&format!(
                "UPDATE node SET version = version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ? RETURNING {NODE_COLUMNS}"
            ))
            .bind(row.id)
            .fetch_one(&mut *connection)
            .await?;
        }
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
        .execute(&mut *connection)
        .await?;
    }
    Ok(created)
}

async fn work_snapshot_items(
    connection: &mut SqliteConnection,
    filter: &WorkFilter,
    only_ready: bool,
    include_done: bool,
    limit: i64,
    offset: i64,
) -> Result<WorkSnapshot, RepositoryError> {
    let rows = work_task_rows(connection, filter).await?;
    let rows: Vec<WorkTaskRow> = rows
        .into_iter()
        .filter(|row| (include_done || !row.done) && (!only_ready || !row.blocked))
        .collect();
    let total = rows.len() as i64;
    let offset = offset.max(0) as usize;
    let limit = limit.max(0) as usize;
    let page: Vec<WorkTaskRow> = rows.into_iter().skip(offset).take(limit).collect();
    let ids: Vec<i64> = page.iter().map(|row| row.id).collect();
    let dependencies = work_dependency_rows(connection, &ids).await?;
    let implementations = work_implementation_rows(connection, &ids).await?;
    Ok(WorkSnapshot {
        rows: page,
        dependencies,
        implementations,
        total,
        limit: limit as i64,
        offset: offset as i64,
    })
}

async fn work_task_rows(
    connection: &mut SqliteConnection,
    filter: &WorkFilter,
) -> Result<Vec<WorkTaskRow>, RepositoryError> {
    let mut builder = QueryBuilder::new(
        "WITH RECURSIVE goal_of(task_id, goal_id) AS ( \
           SELECT g.id, g.id FROM node g \
             JOIN node_kind k ON k.id = g.kind_id AND k.name = 'goal' \
           UNION \
           SELECT e.source, go.goal_id FROM goal_of go \
             JOIN edge e ON e.destination = go.task_id \
             WHERE e.edge_type_id = (SELECT id FROM edge_type WHERE name = 'part_of') \
         ), tasks AS ( \
           SELECT n.id, n.name, n.updated_at, \
             EXISTS(SELECT 1 FROM edge i WHERE i.destination = n.id \
               AND i.edge_type_id = (SELECT id FROM edge_type WHERE name = 'implements')) AS done \
           FROM node n JOIN node_kind k ON k.id = n.kind_id AND k.name = 'task' \
         ), nearest_goal AS ( \
           SELECT task_id, MIN(goal_id) AS goal_id FROM goal_of GROUP BY task_id \
         ) \
         SELECT t.id, t.name, t.updated_at, t.done, \
           (t.done = 0 AND EXISTS(SELECT 1 FROM edge d WHERE d.source = t.id \
             AND d.edge_type_id = (SELECT id FROM edge_type WHERE name = 'depends_on') \
             AND NOT EXISTS(SELECT 1 FROM edge i2 WHERE i2.destination = d.destination \
               AND i2.edge_type_id = (SELECT id FROM edge_type WHERE name = 'implements')))) AS blocked, \
           g.id AS goal_id, g.name AS goal_name \
         FROM tasks t \
         LEFT JOIN nearest_goal ng ON ng.task_id = t.id \
         LEFT JOIN node g ON g.id = ng.goal_id \
         WHERE 1 = 1",
    );
    if let Some(goal) = filter.goal {
        builder.push(
            " AND EXISTS (SELECT 1 FROM goal_of go WHERE go.task_id = t.id AND go.goal_id = ",
        );
        builder.push_bind(goal);
        builder.push(")");
    }
    if let Some(repo) = filter.repo {
        builder.push(
            " AND EXISTS (SELECT 1 FROM goal_of go JOIN edge b ON b.source = go.goal_id \
               AND b.edge_type_id = (SELECT id FROM edge_type WHERE name = 'belongs_to') \
               WHERE go.task_id = t.id AND b.destination = ",
        );
        builder.push_bind(repo);
        builder.push(")");
    }
    builder.push(" ORDER BY t.id");
    let rows = builder
        .build_query_as::<WorkTaskRow>()
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows)
}

async fn work_dependency_rows(
    connection: &mut SqliteConnection,
    task_ids: &[i64],
) -> Result<Vec<WorkDependencyRow>, RepositoryError> {
    if task_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = QueryBuilder::new(
        "SELECT d.source AS task_id, d.destination AS dependency_id, \
           n.name AS dependency_name, \
           EXISTS(SELECT 1 FROM edge i WHERE i.destination = d.destination \
             AND i.edge_type_id = (SELECT id FROM edge_type WHERE name = 'implements')) AS done \
         FROM edge d JOIN node n ON n.id = d.destination \
         WHERE d.edge_type_id = (SELECT id FROM edge_type WHERE name = 'depends_on') \
           AND d.source IN (",
    );
    let mut separated = builder.separated(", ");
    for id in task_ids {
        separated.push_bind(*id);
    }
    builder.push(") ORDER BY d.source, d.destination");
    let rows = builder
        .build_query_as::<WorkDependencyRow>()
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows)
}

async fn work_implementation_rows(
    connection: &mut SqliteConnection,
    task_ids: &[i64],
) -> Result<Vec<WorkImplementationRow>, RepositoryError> {
    if task_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = QueryBuilder::new(
        "SELECT i.destination AS task_id, c.id AS change_id, c.name AS change_name \
         FROM edge i JOIN node c ON c.id = i.source \
         WHERE i.edge_type_id = (SELECT id FROM edge_type WHERE name = 'implements') \
           AND i.destination IN (",
    );
    let mut separated = builder.separated(", ");
    for id in task_ids {
        separated.push_bind(*id);
    }
    builder.push(") ORDER BY i.destination, c.id");
    let rows = builder
        .build_query_as::<WorkImplementationRow>()
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows)
}

fn push_edge_conditions(builder: &mut QueryBuilder<Sqlite>, filter: &EdgeFilter) {
    if let Some(edge_type) = &filter.edge_type {
        builder.push(" AND e.edge_type_id = (SELECT id FROM edge_type WHERE name = ");
        builder.push_bind(edge_type.clone());
        builder.push(")");
    }
    if let Some(source) = filter.source {
        builder.push(" AND e.source = ");
        builder.push_bind(source);
    }
    if let Some(destination) = filter.destination {
        builder.push(" AND e.destination = ");
        builder.push_bind(destination);
    }
}

async fn graph_export_items(
    connection: &mut SqliteConnection,
) -> Result<GraphExport, RepositoryError> {
    let node_rows =
        sqlx::query_as::<_, (i64, i64, String)>("SELECT id, kind_id, name FROM node ORDER BY id")
            .fetch_all(&mut *connection)
            .await?;
    let tag_rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT node_id, tag FROM node_tag ORDER BY node_id, tag",
    )
    .fetch_all(&mut *connection)
    .await?;
    let edge_rows = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT source, destination, edge_type_id FROM edge \
         ORDER BY source, edge_type_id, destination",
    )
    .fetch_all(&mut *connection)
    .await?;

    let mut tags_by_node: HashMap<i64, Vec<String>> = HashMap::new();
    for (node_id, tag) in tag_rows {
        tags_by_node.entry(node_id).or_default().push(tag);
    }
    let nodes = node_rows
        .into_iter()
        .map(|(id, kind_id, name)| GraphExportNode {
            id,
            kind_id,
            name,
            tags: tags_by_node.remove(&id).unwrap_or_default(),
        })
        .collect();
    let edges = edge_rows
        .into_iter()
        .map(|(source, destination, edge_type_id)| GraphExportEdge {
            source,
            destination,
            edge_type_id,
        })
        .collect();
    Ok(GraphExport { nodes, edges })
}

fn push_node_query_conditions(builder: &mut QueryBuilder<Sqlite>, query: &QueryRequest) {
    if query.reach.within.is_some() {
        builder.push(" AND n.id IN (SELECT id FROM reach)");
    }
    if let Some(kind) = &query.kind {
        builder.push(" AND k.name = ");
        builder.push_bind(kind.clone());
    }
    if let Some(name) = &query.name {
        builder.push(" AND n.name = ");
        builder.push_bind(name.clone());
    }
    if let Some(name_contains) = &query.name_contains {
        builder.push(" AND instr(n.name, ");
        builder.push_bind(name_contains.clone());
        builder.push(") > 0");
    }
    if let Some(since) = &query.since {
        builder.push(" AND n.updated_at >= ");
        builder.push_bind(since.clone());
    }
    for tag in &query.tags {
        builder.push(" AND EXISTS(SELECT 1 FROM node_tag qt WHERE qt.node_id = n.id AND qt.tag = ");
        builder.push_bind(tag.clone());
        builder.push(")");
    }
    push_property_filters(builder, &query.properties);
    if let Some(condition) = &query.has_edge {
        push_query_edge_condition(builder, condition, false);
    }
    if let Some(condition) = &query.not_has_edge {
        push_query_edge_condition(builder, condition, true);
    }
}

fn push_query_edge_condition(
    builder: &mut QueryBuilder<Sqlite>,
    condition: &EdgeCondition,
    negate: bool,
) {
    builder.push(if negate {
        " AND NOT EXISTS ("
    } else {
        " AND EXISTS ("
    });
    builder.push(
        "SELECT 1 FROM edge qe JOIN edge_type qt ON qt.id = qe.edge_type_id WHERE qt.name = ",
    );
    builder.push_bind(condition.edge_type.clone());
    builder.push(" AND (");
    match condition.direction {
        QueryDirection::Outgoing => {
            builder.push("qe.source = n.id");
        }
        QueryDirection::Incoming => {
            builder.push("qe.destination = n.id");
        }
        QueryDirection::Both => {
            builder.push("qe.source = n.id OR qe.destination = n.id");
        }
    }
    builder.push("))");
}

fn push_node_query_order(builder: &mut QueryBuilder<Sqlite>, order: &[crate::model::QueryOrder]) {
    builder.push(" ORDER BY ");
    if order.is_empty() {
        builder.push("n.id ASC");
        return;
    }
    for (index, entry) in order.iter().enumerate() {
        if index > 0 {
            builder.push(", ");
        }
        builder.push(match entry.field.as_str() {
            "id" => "n.id",
            "name" => "n.name",
            "updated_at" => "n.updated_at",
            "kind" => "k.name",
            _ => "n.id",
        });
        builder.push(match entry.direction {
            SortDirection::Asc => " ASC",
            SortDirection::Desc => " DESC",
        });
    }
    builder.push(", n.id ASC");
}

fn push_edge_query_conditions(builder: &mut QueryBuilder<Sqlite>, query: &QueryRequest) {
    if let Some(edge_type) = &query.edge_type {
        builder.push(" AND t.name = ");
        builder.push_bind(edge_type.clone());
    }
    if let Some(source) = query.source {
        builder.push(" AND e.source = ");
        builder.push_bind(source);
    }
    if let Some(destination) = query.destination {
        builder.push(" AND e.destination = ");
        builder.push_bind(destination);
    }
    if let Some(since) = &query.since {
        builder.push(" AND e.created_at >= ");
        builder.push_bind(since.clone());
    }
}

fn push_edge_query_order(builder: &mut QueryBuilder<Sqlite>, order: &[crate::model::QueryOrder]) {
    builder.push(" ORDER BY ");
    if order.is_empty() {
        builder.push("e.source ASC, e.edge_type_id ASC, e.destination ASC");
        return;
    }
    for (index, entry) in order.iter().enumerate() {
        if index > 0 {
            builder.push(", ");
        }
        builder.push(match entry.field.as_str() {
            "source" => "e.source",
            "destination" => "e.destination",
            "created_at" => "e.created_at",
            "edge_type" => "t.name",
            _ => "e.source",
        });
        builder.push(match entry.direction {
            SortDirection::Asc => " ASC",
            SortDirection::Desc => " DESC",
        });
    }
    builder.push(", e.source ASC, e.edge_type_id ASC, e.destination ASC");
}

async fn idempotency_reserve_items(
    connection: &mut SqliteConnection,
    key: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<IdempotencyReservation, RepositoryError> {
    sqlx::query(
        "DELETE FROM idempotency_key WHERE status IS NULL \
         AND created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-300 seconds')",
    )
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "DELETE FROM idempotency_key \
         WHERE created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-7 days')",
    )
    .execute(&mut *connection)
    .await?;
    let inserted = sqlx::query(
        "INSERT INTO idempotency_key (key, method, path, request_body) \
         VALUES (?, ?, ?, ?) ON CONFLICT(key) DO NOTHING",
    )
    .bind(key)
    .bind(method)
    .bind(path)
    .bind(body.to_vec())
    .execute(&mut *connection)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(IdempotencyReservation::Reserved);
    }
    let row = sqlx::query_as::<_, (String, String, Vec<u8>, Option<i64>, Option<Vec<u8>>)>(
        "SELECT method, path, request_body, status, response_body \
         FROM idempotency_key WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| RepositoryError::SqlExecutionError("idempotency row vanished".to_string()))?;
    if row.0 != method || row.1 != path || row.2 != body {
        return Ok(IdempotencyReservation::Conflict);
    }
    match (row.3, row.4) {
        (Some(status), Some(response)) => Ok(IdempotencyReservation::Replay {
            status: status as u16,
            body: response,
        }),
        _ => Ok(IdempotencyReservation::InProgress),
    }
}

async fn node_tag_map(
    connection: &mut SqliteConnection,
    ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>, RepositoryError> {
    let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
    if ids.is_empty() {
        return Ok(tags);
    }
    let mut builder = QueryBuilder::new("SELECT node_id, tag FROM node_tag WHERE node_id IN (");
    {
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
    }
    builder.push(") ORDER BY node_id, tag");
    let rows = builder
        .build_query_as::<(i64, String)>()
        .fetch_all(&mut *connection)
        .await?;
    for (node_id, tag) in rows {
        tags.entry(node_id).or_default().push(tag);
    }
    Ok(tags)
}

async fn node_property_map(
    connection: &mut SqliteConnection,
    ids: &[i64],
) -> Result<HashMap<i64, String>, RepositoryError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut builder = QueryBuilder::new("SELECT id, properties FROM node WHERE id IN (");
    {
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
    }
    builder.push(") ORDER BY id");
    let rows = builder
        .build_query_as::<(i64, String)>()
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows.into_iter().collect())
}

async fn query_nodes_items(
    connection: &mut SqliteConnection,
    query: &QueryRequest,
    limit: i64,
    offset: i64,
) -> Result<(NodeQuerySnapshot, i64), RepositoryError> {
    let mut count = QueryBuilder::new("");
    if query.reach.within.is_some() {
        push_reach_cte(&mut count, &query.reach);
    }
    count.push("SELECT count(*) FROM node n JOIN node_kind k ON k.id = n.kind_id WHERE 1 = 1");
    push_node_query_conditions(&mut count, query);
    let total = count
        .build_query_scalar::<i64>()
        .fetch_one(&mut *connection)
        .await?;

    let mut page = QueryBuilder::new("");
    if query.reach.within.is_some() {
        push_reach_cte(&mut page, &query.reach);
    }
    page.push(
        "SELECT n.id, n.kind_id, k.name AS kind, n.name, n.description, n.content, \
         n.updated_at FROM node n JOIN node_kind k ON k.id = n.kind_id WHERE 1 = 1",
    );
    push_node_query_conditions(&mut page, query);
    push_node_query_order(&mut page, &query.order);
    page.push(" LIMIT ");
    page.push_bind(limit);
    page.push(" OFFSET ");
    page.push_bind(offset);
    let rows = page
        .build_query_as::<QueryNodeRow>()
        .fetch_all(&mut *connection)
        .await?;
    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let tags = node_tag_map(connection, &ids).await?;
    let properties = node_property_map(connection, &ids).await?;
    Ok((
        NodeQuerySnapshot {
            rows,
            tags,
            properties,
        },
        total,
    ))
}

async fn query_edges_items(
    connection: &mut SqliteConnection,
    query: &QueryRequest,
    limit: i64,
    offset: i64,
) -> Result<(Vec<QueryEdgeRow>, i64), RepositoryError> {
    let mut count = QueryBuilder::new(
        "SELECT count(*) FROM edge e JOIN edge_type t ON t.id = e.edge_type_id WHERE 1 = 1",
    );
    push_edge_query_conditions(&mut count, query);
    let total = count
        .build_query_scalar::<i64>()
        .fetch_one(&mut *connection)
        .await?;

    let mut page = QueryBuilder::new(
        "SELECT e.source, e.destination, e.edge_type_id, t.name AS edge_type, \
         e.created_at, e.properties FROM edge e JOIN edge_type t ON t.id = e.edge_type_id \
         WHERE 1 = 1",
    );
    push_edge_query_conditions(&mut page, query);
    push_edge_query_order(&mut page, &query.order);
    page.push(" LIMIT ");
    page.push_bind(limit);
    page.push(" OFFSET ");
    page.push_bind(offset);
    let rows = page
        .build_query_as::<QueryEdgeRow>()
        .fetch_all(&mut *connection)
        .await?;
    Ok((rows, total))
}

async fn node_batch_items(
    connection: &mut SqliteConnection,
    ids: &[i64],
) -> Result<NodeBatchSnapshot, RepositoryError> {
    let mut node_query =
        QueryBuilder::new(format!("SELECT {NODE_COLUMNS} FROM node WHERE id IN ("));
    {
        let mut separated = node_query.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
    }
    node_query.push(") ORDER BY id");
    let nodes = node_query
        .build_query_as::<Node>()
        .fetch_all(&mut *connection)
        .await?;

    let mut tag_query = QueryBuilder::new("SELECT node_id, tag FROM node_tag WHERE node_id IN (");
    {
        let mut separated = tag_query.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
    }
    tag_query.push(") ORDER BY node_id, tag");
    let tag_rows = tag_query
        .build_query_as::<(i64, String)>()
        .fetch_all(&mut *connection)
        .await?;
    let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
    for (node_id, tag) in tag_rows {
        tags.entry(node_id).or_default().push(tag);
    }

    let mut property_query = QueryBuilder::new("SELECT id, properties FROM node WHERE id IN (");
    {
        let mut separated = property_query.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
    }
    property_query.push(") ORDER BY id");
    let properties = property_query
        .build_query_as::<(i64, String)>()
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect();

    Ok(NodeBatchSnapshot {
        nodes,
        tags,
        properties,
    })
}

async fn node_tags_in(
    connection: &mut SqliteConnection,
    node_id: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT tag FROM node_tag WHERE node_id = ? ORDER BY tag")
        .bind(node_id)
        .fetch_all(&mut *connection)
        .await
}

fn tags_equal(current: &[String], proposed: &[String]) -> bool {
    let mut current = current.to_vec();
    let mut proposed = proposed.to_vec();
    current.sort();
    proposed.sort();
    current == proposed
}

fn push_reach_cte(builder: &mut QueryBuilder<Sqlite>, reach: &ReachFilter) {
    builder.push(
        "WITH RECURSIVE reach(id) AS ( \
         SELECT e.source FROM edge e WHERE e.destination = ",
    );
    builder.push_bind(reach.within);
    builder.push(" AND (");
    builder.push_bind(reach.via.clone());
    builder.push(" IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ");
    builder.push_bind(reach.via.clone());
    builder.push(")) UNION SELECT e.destination FROM edge e WHERE e.source = ");
    builder.push_bind(reach.within);
    builder.push(" AND (");
    builder.push_bind(reach.via.clone());
    builder.push(" IS NULL OR e.edge_type_id = (SELECT id FROM edge_type WHERE name = ");
    builder.push_bind(reach.via.clone());
    builder.push(
        ")) UNION SELECT e.source FROM edge e JOIN reach r ON e.destination = r.id \
         WHERE e.edge_type_id = (SELECT id FROM edge_type WHERE name = ",
    );
    builder.push_bind(reach.descend.clone());
    builder.push(")) ");
}

fn push_node_conditions(
    builder: &mut QueryBuilder<Sqlite>,
    filter: &NodeFilter,
    since: Option<&str>,
) {
    if filter.reach.within.is_some() {
        builder.push(" AND n.id IN (SELECT id FROM reach)");
    }
    builder.push(" AND (");
    builder.push_bind(filter.kind.clone());
    builder.push(" IS NULL OR n.kind_id = (SELECT id FROM node_kind WHERE name = ");
    builder.push_bind(filter.kind.clone());
    builder.push("))");
    if let Some(name) = &filter.name {
        builder.push(" AND n.name = ");
        builder.push_bind(name.clone());
    }
    if let Some(name_contains) = &filter.name_contains {
        builder.push(" AND instr(n.name, ");
        builder.push_bind(name_contains.clone());
        builder.push(") > 0");
    }
    if let Some(since) = since {
        builder.push(" AND n.updated_at >= ");
        builder.push_bind(since.to_string());
    }
    builder.push(scope_clause(filter.scope));
    push_property_filters(builder, &filter.properties);
}

fn push_property_filters(
    builder: &mut QueryBuilder<Sqlite>,
    properties: &serde_json::Map<String, serde_json::Value>,
) {
    for (key, value) in properties {
        let path = json_path_literal(key);
        match value {
            serde_json::Value::Null => {
                builder.push(format!(" AND json_type(n.properties, '{path}') = 'null'"));
            }
            serde_json::Value::Bool(value) => {
                builder.push(format!(" AND json_extract(n.properties, '{path}') = "));
                builder.push_bind(if *value { 1i64 } else { 0i64 });
            }
            serde_json::Value::Number(number) => {
                builder.push(format!(" AND json_extract(n.properties, '{path}') = "));
                if let Some(integer) = number.as_i64() {
                    builder.push_bind(integer);
                } else {
                    builder.push_bind(number.as_f64().unwrap_or_default());
                }
            }
            serde_json::Value::String(value) => {
                builder.push(format!(" AND json_extract(n.properties, '{path}') = "));
                builder.push_bind(value.clone());
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                builder.push(format!(" AND json_extract(n.properties, '{path}') = json("));
                builder.push_bind(serde_json::to_string(value).unwrap_or_default());
                builder.push(")");
            }
        }
    }
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

async fn upsert_node_with_tags_items(
    connection: &mut SqliteConnection,
    node: &Node,
    tags: &[String],
    properties: Option<&serde_json::Value>,
) -> Result<Node, RepositoryError> {
    let schema = load_kind_schema(connection, node.kind_id).await?;
    let empty = serde_json::json!({});
    let candidate = properties.unwrap_or(&empty);
    if properties.is_some() || node.id == 0 {
        validate_declared_properties(candidate, &schema.properties, schema.strict)
            .map_err(RepositoryError::ContractViolation)?;
    }
    let row = insert_node(&mut *connection, node)
        .await
        .map_err(RepositoryError::from)?;
    replace_tags(connection, row.id, tags).await?;
    if let Some(properties) = properties {
        let encoded = serde_json::to_string(properties)
            .map_err(|error| RepositoryError::SqlExecutionError(error.to_string()))?;
        if node.id == 0 {
            sqlx::query("UPDATE node SET properties = ? WHERE id = ?")
                .bind(encoded)
                .bind(row.id)
                .execute(&mut *connection)
                .await?;
        } else {
            sqlx::query(
                "UPDATE node SET properties = ?, version = version + 1, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
            )
            .bind(encoded)
            .bind(row.id)
            .execute(&mut *connection)
            .await?;
        }
    }
    let row = sqlx::query_as::<_, Node>(&format!("SELECT {NODE_COLUMNS} FROM node WHERE id = ?"))
        .bind(row.id)
        .fetch_one(&mut *connection)
        .await?;
    Ok(row)
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
             version = CASE WHEN node.kind_id = excluded.kind_id \
               AND node.name = excluded.name AND node.description = excluded.description \
               AND node.content = excluded.content THEN node.version ELSE node.version + 1 END, \
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
