use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct NodeKind {
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub description: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct EdgeType {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub resolves: bool,
    pub single_outgoing: bool,
    pub source_kinds: Option<String>,
    pub destination_kinds: Option<String>,
    pub updated_at: String,
}

impl EdgeType {
    pub fn source_kind_list(&self) -> Vec<&str> {
        split_kind_list(self.source_kinds.as_deref())
    }

    pub fn destination_kind_list(&self) -> Vec<&str> {
        split_kind_list(self.destination_kinds.as_deref())
    }
}

pub(crate) fn split_kind_list(kinds: Option<&str>) -> Vec<&str> {
    kinds
        .map(|kinds| {
            kinds
                .split(',')
                .map(str::trim)
                .filter(|kind| !kind.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn encode_kind_list(kinds: &[String]) -> Option<String> {
    let mut unique: Vec<&str> = Vec::new();
    for kind in kinds {
        let kind = kind.trim();
        if !kind.is_empty() && !unique.contains(&kind) {
            unique.push(kind);
        }
    }
    (!unique.is_empty()).then(|| unique.join(","))
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Node {
    pub id: i64,
    pub kind_id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Edge {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationDirection {
    Outgoing,
    Ingoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborDirection {
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    Active,
    All,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReachFilter {
    pub within: Option<i64>,
    pub via: Option<String>,
    pub descend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFilter {
    pub kind: Option<String>,
    pub scope: SearchScope,
    pub reach: ReachFilter,
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            kind: None,
            scope: SearchScope::All,
            reach: ReachFilter::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub direction: RelationDirection,
    pub edge: Edge,
    pub node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub node: Node,
    pub score: f64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct TagUsage {
    pub tag: String,
    pub node_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackStatus {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackItem {
    pub id: i64,
    pub name: String,
    pub version: Option<i64>,
    pub status: PackStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackNodeKind {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEdgeType {
    pub name: String,
    pub description: Option<String>,
    pub resolves: Option<bool>,
    pub single_outgoing: Option<bool>,
    pub source_kinds: Option<Vec<String>>,
    pub destination_kinds: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackDraft {
    pub node_kinds: Vec<PackNodeKind>,
    pub edge_types: Vec<PackEdgeType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackReport {
    pub node_kinds: Vec<PackItem>,
    pub edge_types: Vec<PackItem>,
}

impl PackReport {
    pub fn created(&self) -> usize {
        self.items()
            .filter(|item| item.status == PackStatus::Created)
            .count()
    }

    pub fn updated(&self) -> usize {
        self.items()
            .filter(|item| item.status == PackStatus::Updated)
            .count()
    }

    pub fn unchanged(&self) -> usize {
        self.items()
            .filter(|item| item.status == PackStatus::Unchanged)
            .count()
    }

    fn items(&self) -> impl Iterator<Item = &PackItem> {
        self.node_kinds.iter().chain(self.edge_types.iter())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct KindUsage {
    pub kind_id: i64,
    pub kind: String,
    pub node_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct EdgeTypeUsage {
    pub edge_type_id: i64,
    pub edge_type: String,
    pub edge_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub kind_id: i64,
    pub kind: String,
    pub name: String,
    pub node_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct IsolatedNode {
    pub id: i64,
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphHealth {
    pub node_count: i64,
    pub edge_count: i64,
    pub kinds: Vec<KindUsage>,
    pub edge_types: Vec<EdgeTypeUsage>,
    pub duplicates: Vec<DuplicateGroup>,
    pub isolated_count: i64,
    pub isolated: Vec<IsolatedNode>,
    pub unanchored_count: i64,
    pub unanchored: Vec<IsolatedNode>,
}

impl GraphHealth {
    pub fn dead_kinds(&self) -> impl Iterator<Item = &KindUsage> {
        self.kinds.iter().filter(|usage| usage.node_count == 0)
    }

    pub fn underused_edge_types(&self) -> impl Iterator<Item = &EdgeTypeUsage> {
        self.edge_types.iter().filter(|usage| usage.edge_count <= 1)
    }

    pub fn summary_line(&self) -> String {
        format!(
            "nodes={} edges={} kinds={} (dead={}) edge_types={} (underused={}) duplicate_groups={} isolated_nodes={} unanchored_work={}",
            self.node_count,
            self.edge_count,
            self.kinds.len(),
            self.dead_kinds().count(),
            self.edge_types.len(),
            self.underused_edge_types().count(),
            self.duplicates.len(),
            self.isolated_count,
            self.unanchored_count,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRef {
    Existing(i64),
    New(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEdge {
    pub source: NodeRef,
    pub destination: NodeRef,
    pub edge_type_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDraft {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
    pub parent: Option<i64>,
    pub depends_on: Vec<i64>,
    pub affects: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
}

impl TaskPatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.description.is_none() && self.content.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanGoalDraft {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTaskDraft {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub parent: Option<String>,
    pub depends_on: Vec<String>,
    pub affects: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDraft {
    pub goal: Option<PlanGoalDraft>,
    pub goal_id: Option<i64>,
    pub anchor: Option<PlanAnchorDraft>,
    pub tasks: Vec<PlanTaskDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanAnchorDraft {
    pub node_id: i64,
    pub edge_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTask {
    pub key: String,
    pub node: Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub goal: Option<Node>,
    pub tasks: Vec<PlanTask>,
}
