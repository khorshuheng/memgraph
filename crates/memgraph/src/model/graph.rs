use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct NodeKind {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct EdgeType {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Node {
    pub id: i64,
    pub kind_id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Edge {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
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

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentMatches {
    pub segment: String,
    pub matches: Vec<SearchHit>,
    pub total_matches: usize,
}
