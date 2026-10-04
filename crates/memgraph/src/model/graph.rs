use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct NodeKind {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct EdgeType {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub updated_at: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationSummary {
    pub direction: RelationDirection,
    pub edge_type_id: i64,
    pub edge_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub node: Node,
    pub confidence: f64,
    pub matched_terms: Vec<String>,
    pub relations: Vec<RelationSummary>,
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
            "nodes={} edges={} kinds={} (dead={}) edge_types={} (underused={}) duplicate_groups={} isolated_nodes={}",
            self.node_count,
            self.edge_count,
            self.kinds.len(),
            self.dead_kinds().count(),
            self.edge_types.len(),
            self.underused_edge_types().count(),
            self.duplicates.len(),
            self.isolated_count,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentMatches {
    pub segment: String,
    pub matches: Vec<SearchHit>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessAction {
    Read,
    Write,
}

impl AccessAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccessAction::Read => "read",
            AccessAction::Write => "write",
        }
    }
}
