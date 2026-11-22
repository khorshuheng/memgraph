use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct NodeKind {
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub description: String,
    pub unique_name: bool,
    pub strict_properties: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct EdgeType {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub resolves: bool,
    pub single_outgoing: bool,
    pub acyclic: bool,
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

#[derive(Debug, Clone, PartialEq)]
pub struct NodeFilter {
    pub kind: Option<String>,
    pub scope: SearchScope,
    pub reach: ReachFilter,
    pub properties: serde_json::Map<String, serde_json::Value>,
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            kind: None,
            scope: SearchScope::All,
            reach: ReachFilter::default(),
            properties: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodePage {
    pub items: Vec<Node>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub direction: RelationDirection,
    pub edge: Edge,
    pub node: Node,
    pub edge_properties: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkFilter {
    pub goal: Option<i64>,
    pub repo: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkStatus {
    Pending,
    Blocked,
    Done,
}

impl WorkStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkStatus::Pending => "pending",
            WorkStatus::Blocked => "blocked",
            WorkStatus::Done => "done",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRef {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDependency {
    pub id: i64,
    pub name: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkTask {
    pub id: i64,
    pub name: String,
    pub status: WorkStatus,
    pub updated_at: String,
    pub goal_id: Option<i64>,
    pub goal_name: Option<String>,
    pub depends_on: Vec<WorkDependency>,
    pub implemented_by: Vec<WorkRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkPage {
    pub items: Vec<WorkTask>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdgeFilter {
    pub edge_type: Option<String>,
    pub source: Option<i64>,
    pub destination: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct EdgeRecord {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
    pub created_at: String,
    pub properties: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgePage {
    pub items: Vec<EdgeRecord>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExportNode {
    pub id: i64,
    pub kind_id: i64,
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExportEdge {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExport {
    pub nodes: Vec<GraphExportNode>,
    pub edges: Vec<GraphExportEdge>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PropertyType {
    #[default]
    Any,
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
}

impl PropertyType {
    pub fn as_str(self) -> &'static str {
        match self {
            PropertyType::Any => "any",
            PropertyType::String => "string",
            PropertyType::Number => "number",
            PropertyType::Integer => "integer",
            PropertyType::Boolean => "boolean",
            PropertyType::Array => "array",
            PropertyType::Object => "object",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "any" => Some(PropertyType::Any),
            "string" => Some(PropertyType::String),
            "number" => Some(PropertyType::Number),
            "integer" => Some(PropertyType::Integer),
            "boolean" => Some(PropertyType::Boolean),
            "array" => Some(PropertyType::Array),
            "object" => Some(PropertyType::Object),
            _ => None,
        }
    }

    pub fn accepts(self, value: &serde_json::Value) -> bool {
        match self {
            PropertyType::Any => true,
            PropertyType::String => value.is_string(),
            PropertyType::Number => value.is_number(),
            PropertyType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
            PropertyType::Boolean => value.is_boolean(),
            PropertyType::Array => value.is_array(),
            PropertyType::Object => value.is_object(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeKindProperty {
    pub key: String,
    pub value_type: PropertyType,
    pub required: bool,
    pub indexed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackProperty {
    pub key: String,
    pub value_type: Option<String>,
    pub required: bool,
    pub indexed: bool,
}

pub fn validate_declared_properties(
    properties: &serde_json::Value,
    declarations: &[NodeKindProperty],
    strict: bool,
) -> Result<(), String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "properties must be a JSON object".to_string())?;
    if strict {
        for key in object.keys() {
            if !declarations
                .iter()
                .any(|declaration| declaration.key == *key)
            {
                return Err(format!("property '{key}' is not declared for this kind"));
            }
        }
    }
    for declaration in declarations {
        match object.get(&declaration.key) {
            Some(value) => {
                if !declaration.value_type.accepts(value) {
                    return Err(format!(
                        "property '{}' must be a {}",
                        declaration.key,
                        declaration.value_type.as_str()
                    ));
                }
            }
            None if declaration.required => {
                return Err(format!("property '{}' is required", declaration.key));
            }
            None => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackNodeKind {
    pub name: String,
    pub description: Option<String>,
    pub unique_name: Option<bool>,
    pub strict_properties: Option<bool>,
    pub properties: Option<Vec<PackProperty>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEdgeType {
    pub name: String,
    pub description: Option<String>,
    pub resolves: Option<bool>,
    pub single_outgoing: Option<bool>,
    pub acyclic: Option<bool>,
    pub source_kinds: Option<Vec<String>>,
    pub destination_kinds: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackDraft {
    pub node_kinds: Vec<PackNodeKind>,
    pub edge_types: Vec<PackEdgeType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportConflict {
    #[default]
    Update,
    Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportNode {
    pub reference: Option<String>,
    pub id: Option<i64>,
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRef {
    Id(i64),
    Ref(String),
    NaturalKey { kind: String, name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportEdge {
    pub source: ImportRef,
    pub destination: ImportRef,
    pub edge_type: String,
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    Created,
    Updated,
    Unchanged,
    Skipped,
    Existing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportNodeResult {
    pub reference: Option<String>,
    pub id: i64,
    pub status: ImportStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEdgeResult {
    pub source: i64,
    pub destination: i64,
    pub edge_type: String,
    pub status: ImportStatus,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportDraft {
    pub on_conflict: ImportConflict,
    pub nodes: Vec<ImportNode>,
    pub edges: Vec<ImportEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub nodes: Vec<ImportNodeResult>,
    pub edges: Vec<ImportEdgeResult>,
}

impl ImportReport {
    pub fn nodes_created(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.status == ImportStatus::Created)
            .count()
    }

    pub fn nodes_updated(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.status == ImportStatus::Updated)
            .count()
    }

    pub fn nodes_unchanged(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.status == ImportStatus::Unchanged)
            .count()
    }

    pub fn nodes_skipped(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.status == ImportStatus::Skipped)
            .count()
    }

    pub fn edges_created(&self) -> usize {
        self.edges
            .iter()
            .filter(|edge| edge.status == ImportStatus::Created)
            .count()
    }

    pub fn edges_existing(&self) -> usize {
        self.edges
            .iter()
            .filter(|edge| edge.status == ImportStatus::Existing)
            .count()
    }
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
            "nodes={} edges={} kinds={} (dead={}) edge_types={} (underused={}) isolated_nodes={} unanchored_work={}",
            self.node_count,
            self.edge_count,
            self.kinds.len(),
            self.dead_kinds().count(),
            self.edge_types.len(),
            self.underused_edge_types().count(),
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

#[derive(Debug, Clone, PartialEq)]
pub struct PendingNode {
    pub node: Node,
    pub properties: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEdge {
    pub source: NodeRef,
    pub destination: NodeRef,
    pub edge_type_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeDraft {
    pub commit: String,
    pub title: String,
    pub summary: String,
    pub tasks: Vec<i64>,
    pub files: Vec<String>,
    pub properties: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
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
    pub properties: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub properties: Option<serde_json::Value>,
}

impl NodePatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.content.is_none()
            && self.tags.is_none()
            && self.properties.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
    pub properties: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
}

impl TaskPatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.content.is_none()
            && self.properties.is_none()
            && self.tags.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanGoalDraft {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
    pub properties: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
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
    pub properties: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
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
