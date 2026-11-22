use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::model::{
    Edge as EdgeModel, EdgeType as EdgeTypeModel, ImportConflict, ImportDraft, ImportEdge,
    ImportNode, ImportRef, ImportReport, ImportStatus, Neighbor as NeighborModel,
    NeighborDirection as NeighborDirectionModel, Node as NodeModel, NodeFilter,
    NodeKind as NodeKindModel, NodeKindProperty, NodePatch, PackDraft, PackEdgeType, PackNodeKind,
    PackReport, Plan as PlanModel, PlanAnchorDraft, PlanDraft, PlanGoalDraft,
    PlanTask as PlanTaskModel, PlanTaskDraft, ReachFilter,
    RelationDirection as RelationDirectionModel, SearchScope as SearchScopeModel, TaskDraft,
    TaskPatch,
};

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A node kind. strict_properties rejects property keys that are not \
                   declared for the kind; it defaults to open, and a strict kind with no \
                   declarations accepts only {}."
)]
pub struct CreateNodeKindParameters {
    pub name: String,
    pub description: String,
    #[serde(default = "default_unique_name")]
    #[schema(default = true)]
    pub unique_name: bool,
    #[serde(default)]
    #[schema(default = false)]
    pub strict_properties: bool,
}

fn default_unique_name() -> bool {
    true
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NodeKindResponse {
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub description: String,
    pub unique_name: bool,
    pub strict_properties: bool,
    pub properties: Vec<NodeKindPropertyResponse>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(
    description = "A declared property. indexed means a single-column expression index \
                   backs exact-match property filters."
)]
pub struct NodeKindPropertyResponse {
    pub key: String,
    pub value_type: String,
    pub required: bool,
    pub indexed: bool,
}

impl From<NodeKindProperty> for NodeKindPropertyResponse {
    fn from(property: NodeKindProperty) -> Self {
        NodeKindPropertyResponse {
            key: property.key,
            value_type: property.value_type.as_str().to_string(),
            required: property.required,
            indexed: property.indexed,
        }
    }
}

impl From<CreateNodeKindParameters> for NodeKindModel {
    fn from(params: CreateNodeKindParameters) -> Self {
        NodeKindModel {
            id: 0,
            version: 1,
            name: params.name,
            description: params.description,
            unique_name: params.unique_name,
            strict_properties: params.strict_properties,
            updated_at: String::new(),
        }
    }
}

impl NodeKindResponse {
    pub fn with_properties(
        node_kind: NodeKindModel,
        properties: Vec<NodeKindPropertyResponse>,
    ) -> Self {
        NodeKindResponse {
            id: node_kind.id,
            version: node_kind.version,
            name: node_kind.name,
            description: node_kind.description,
            unique_name: node_kind.unique_name,
            strict_properties: node_kind.strict_properties,
            properties,
            updated_at: node_kind.updated_at,
        }
    }
}

impl From<NodeKindModel> for NodeKindResponse {
    fn from(node_kind: NodeKindModel) -> Self {
        NodeKindResponse::with_properties(node_kind, Vec::new())
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEdgeTypeParameters {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub resolves: bool,
    #[serde(default)]
    pub single_outgoing: bool,
    #[serde(default)]
    pub acyclic: bool,
    #[serde(default)]
    pub source_kinds: Vec<String>,
    #[serde(default)]
    pub destination_kinds: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeTypeResponse {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub resolves: bool,
    pub single_outgoing: bool,
    pub acyclic: bool,
    pub source_kinds: Vec<String>,
    pub destination_kinds: Vec<String>,
    pub updated_at: String,
}

fn join_kinds(kinds: Vec<String>) -> Option<String> {
    let kinds: Vec<String> = kinds
        .into_iter()
        .map(|kind| kind.trim().to_string())
        .filter(|kind| !kind.is_empty())
        .collect();
    (!kinds.is_empty()).then(|| kinds.join(","))
}

fn split_kinds(kinds: Option<&str>) -> Vec<String> {
    kinds
        .map(|kinds| {
            kinds
                .split(',')
                .map(|kind| kind.trim().to_string())
                .filter(|kind| !kind.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl From<CreateEdgeTypeParameters> for EdgeTypeModel {
    fn from(params: CreateEdgeTypeParameters) -> Self {
        EdgeTypeModel {
            id: 0,
            name: params.name,
            description: params.description,
            resolves: params.resolves,
            single_outgoing: params.single_outgoing,
            acyclic: params.acyclic,
            source_kinds: join_kinds(params.source_kinds),
            destination_kinds: join_kinds(params.destination_kinds),
            updated_at: String::new(),
        }
    }
}

impl From<EdgeTypeModel> for EdgeTypeResponse {
    fn from(edge_type: EdgeTypeModel) -> Self {
        EdgeTypeResponse {
            source_kinds: split_kinds(edge_type.source_kinds.as_deref()),
            destination_kinds: split_kinds(edge_type.destination_kinds.as_deref()),
            id: edge_type.id,
            name: edge_type.name,
            description: edge_type.description,
            resolves: edge_type.resolves,
            single_outgoing: edge_type.single_outgoing,
            acyclic: edge_type.acyclic,
            updated_at: edge_type.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateNodeParameters {
    pub kind_id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
}

impl From<CreateNodeParameters> for NodeModel {
    fn from(params: CreateNodeParameters) -> Self {
        NodeModel {
            id: 0,
            kind_id: params.kind_id,
            name: params.name,
            description: params.description,
            content: params.content,
            updated_at: String::new(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NodeResponse {
    pub id: i64,
    pub kind_id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
    pub tags: Vec<String>,
    pub properties: serde_json::Value,
    pub updated_at: String,
}

impl NodeResponse {
    pub fn with_tags_and_properties(
        node: NodeModel,
        tags: Vec<String>,
        properties: serde_json::Value,
    ) -> Self {
        NodeResponse {
            id: node.id,
            kind_id: node.kind_id,
            name: node.name,
            description: node.description,
            content: node.content,
            tags,
            properties,
            updated_at: node.updated_at,
        }
    }
}

impl From<NodeModel> for NodeResponse {
    fn from(node: NodeModel) -> Self {
        NodeResponse {
            id: node.id,
            kind_id: node.kind_id,
            name: node.name,
            description: node.description,
            content: node.content,
            tags: Vec::new(),
            properties: serde_json::json!({}),
            updated_at: node.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEdgeParameters {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeResponse {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
    pub properties: serde_json::Value,
    pub created_at: String,
}

impl From<CreateEdgeParameters> for EdgeModel {
    fn from(params: CreateEdgeParameters) -> Self {
        EdgeModel {
            source: params.source,
            destination: params.destination,
            edge_type_id: params.edge_type_id,
            created_at: String::new(),
        }
    }
}

impl From<EdgeModel> for EdgeResponse {
    fn from(edge: EdgeModel) -> Self {
        EdgeResponse {
            source: edge.source,
            destination: edge.destination,
            edge_type_id: edge.edge_type_id,
            properties: serde_json::json!({}),
            created_at: edge.created_at,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeleteEdgeParameters {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
}

impl From<DeleteEdgeParameters> for EdgeModel {
    fn from(params: DeleteEdgeParameters) -> Self {
        EdgeModel {
            source: params.source,
            destination: params.destination,
            edge_type_id: params.edge_type_id,
            created_at: String::new(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateEdgeParameters {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NeighborDirection {
    #[default]
    Outgoing,
    Incoming,
    Both,
}

impl From<NeighborDirection> for NeighborDirectionModel {
    fn from(direction: NeighborDirection) -> Self {
        match direction {
            NeighborDirection::Outgoing => NeighborDirectionModel::Outgoing,
            NeighborDirection::Incoming => NeighborDirectionModel::Incoming,
            NeighborDirection::Both => NeighborDirectionModel::Both,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NeighborParameters {
    #[serde(default)]
    pub direction: NeighborDirection,
    pub edge_type: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NeighborResponse {
    pub direction: String,
    pub edge: EdgeResponse,
    pub node: NodeResponse,
}

impl NeighborResponse {
    pub fn with_tags_and_properties(
        neighbor: NeighborModel,
        tags: Vec<String>,
        properties: serde_json::Value,
    ) -> Self {
        let edge_properties = serde_json::from_str(&neighbor.edge_properties)
            .unwrap_or_else(|_| serde_json::json!({}));
        NeighborResponse {
            direction: match neighbor.direction {
                RelationDirectionModel::Outgoing => "outgoing".to_string(),
                RelationDirectionModel::Ingoing => "incoming".to_string(),
            },
            edge: EdgeResponse {
                properties: edge_properties,
                ..neighbor.edge.into()
            },
            node: NodeResponse::with_tags_and_properties(neighbor.node, tags, properties),
        }
    }
}

impl From<NeighborModel> for NeighborResponse {
    fn from(neighbor: NeighborModel) -> Self {
        NeighborResponse::with_tags_and_properties(neighbor, Vec::new(), serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    #[default]
    Active,
    All,
    Resolved,
}

impl From<SearchScope> for SearchScopeModel {
    fn from(scope: SearchScope) -> Self {
        match scope {
            SearchScope::Active => SearchScopeModel::Active,
            SearchScope::All => SearchScopeModel::All,
            SearchScope::Resolved => SearchScopeModel::Resolved,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListNodesParameters {
    pub kind: Option<String>,
    pub scope: Option<SearchScope>,
    pub within: Option<i64>,
    pub via: Option<String>,
    pub descend: Option<String>,
    pub properties: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListNodesResponse {
    pub items: Vec<NodeResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

impl ListNodesParameters {
    pub fn property_filter(&self) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        parse_property_filter(self.properties.as_deref())
    }
}

impl From<ListNodesParameters> for NodeFilter {
    fn from(params: ListNodesParameters) -> Self {
        NodeFilter {
            kind: params.kind,
            scope: params.scope.unwrap_or(SearchScope::All).into(),
            reach: ReachFilter {
                within: params.within,
                via: params.via,
                descend: params.descend,
            },
            properties: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A node kind declaration. unique_name controls name uniqueness and \
                   strict_properties rejects undeclared property keys; omitted fields keep \
                   the stored value."
)]
pub struct PackNodeKindParameters {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub unique_name: Option<bool>,
    #[serde(default)]
    pub strict_properties: Option<bool>,
    #[serde(default)]
    pub properties: Option<Vec<PackPropertyParameters>>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PackPropertyType {
    #[default]
    Any,
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
}

impl PackPropertyType {
    pub fn as_str(self) -> &'static str {
        match self {
            PackPropertyType::Any => "any",
            PackPropertyType::String => "string",
            PackPropertyType::Number => "number",
            PackPropertyType::Integer => "integer",
            PackPropertyType::Boolean => "boolean",
            PackPropertyType::Array => "array",
            PackPropertyType::Object => "object",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A declared property. indexed=true maintains a single-column expression \
                   index used by exact-match property filters. Keys must not contain a double quote."
)]
pub struct PackPropertyParameters {
    pub key: String,
    #[serde(default)]
    pub value_type: PackPropertyType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub indexed: bool,
}

impl From<PackNodeKindParameters> for PackNodeKind {
    fn from(params: PackNodeKindParameters) -> Self {
        PackNodeKind {
            name: params.name,
            description: params.description,
            unique_name: params.unique_name,
            strict_properties: params.strict_properties,
            properties: params.properties.map(|properties| {
                properties
                    .into_iter()
                    .map(|property| crate::model::PackProperty {
                        key: property.key,
                        value_type: Some(property.value_type.as_str().to_string()),
                        required: property.required,
                        indexed: property.indexed,
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PackEdgeTypeParameters {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub resolves: Option<bool>,
    #[serde(default)]
    pub single_outgoing: Option<bool>,
    #[serde(default)]
    pub acyclic: Option<bool>,
    #[serde(default)]
    pub source_kinds: Option<Vec<String>>,
    #[serde(default)]
    pub destination_kinds: Option<Vec<String>>,
}

impl From<PackEdgeTypeParameters> for PackEdgeType {
    fn from(params: PackEdgeTypeParameters) -> Self {
        PackEdgeType {
            name: params.name,
            description: params.description,
            resolves: params.resolves,
            single_outgoing: params.single_outgoing,
            acyclic: params.acyclic,
            source_kinds: params.source_kinds,
            destination_kinds: params.destination_kinds,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "Vocabulary pack. Names are matched exactly (case-sensitive). \
                   Omitted node-kind fields are left unchanged; omitted edge-type fields \
                   keep their stored value, and an empty list clears a constraint. \
                   Nothing is deleted."
)]
pub struct ApplyPackParameters {
    #[serde(default)]
    pub node_kinds: Vec<PackNodeKindParameters>,
    #[serde(default)]
    pub edge_types: Vec<PackEdgeTypeParameters>,
}

impl From<ApplyPackParameters> for PackDraft {
    fn from(params: ApplyPackParameters) -> Self {
        PackDraft {
            node_kinds: params.node_kinds.into_iter().map(Into::into).collect(),
            edge_types: params.edge_types.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackItemResponse {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackReportResponse {
    pub node_kinds: Vec<PackItemResponse>,
    pub edge_types: Vec<PackItemResponse>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
}

impl From<PackReport> for PackReportResponse {
    fn from(report: PackReport) -> Self {
        PackReportResponse {
            created: report.created(),
            updated: report.updated(),
            unchanged: report.unchanged(),
            node_kinds: report.node_kinds.into_iter().map(Into::into).collect(),
            edge_types: report.edge_types.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::model::PackItem> for PackItemResponse {
    fn from(item: crate::model::PackItem) -> Self {
        use crate::model::PackStatus;
        PackItemResponse {
            id: item.id,
            name: item.name,
            version: item.version,
            status: match item.status {
                PackStatus::Created => "created".to_string(),
                PackStatus::Updated => "updated".to_string(),
                PackStatus::Unchanged => "unchanged".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImportConflictParameters {
    #[default]
    Update,
    Skip,
}

impl From<ImportConflictParameters> for ImportConflict {
    fn from(params: ImportConflictParameters) -> Self {
        match params {
            ImportConflictParameters::Update => ImportConflict::Update,
            ImportConflictParameters::Skip => ImportConflict::Skip,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ImportNodeRefParameters {
    NaturalKey { kind: String, name: String },
    Id(i64),
    Ref(String),
}

impl From<ImportNodeRefParameters> for ImportRef {
    fn from(params: ImportNodeRefParameters) -> Self {
        match params {
            ImportNodeRefParameters::Id(id) => ImportRef::Id(id),
            ImportNodeRefParameters::Ref(reference) => ImportRef::Ref(reference),
            ImportNodeRefParameters::NaturalKey { kind, name } => {
                ImportRef::NaturalKey { kind, name }
            }
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportNodeParameters {
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    #[serde(default)]
    pub id: Option<i64>,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
}

impl From<ImportNodeParameters> for ImportNode {
    fn from(params: ImportNodeParameters) -> Self {
        ImportNode {
            reference: params.reference,
            id: params.id,
            kind: params.kind,
            name: params.name,
            description: params.description,
            content: params.content,
            tags: params.tags,
            properties: params.properties,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportEdgeParameters {
    pub source: ImportNodeRefParameters,
    pub destination: ImportNodeRefParameters,
    #[serde(rename = "type")]
    pub edge_type: String,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
}

impl From<ImportEdgeParameters> for ImportEdge {
    fn from(params: ImportEdgeParameters) -> Self {
        ImportEdge {
            source: params.source.into(),
            destination: params.destination.into(),
            edge_type: params.edge_type,
            properties: params.properties,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "Batch import. Nodes are identified by (kind, name) unless an id is given: \
                   id targets that exact node, creating it if absent, and kind is immutable on \
                   update. Omitted description/content/tags are left unchanged. Edge endpoints \
                   are a numeric node id, a request-local ref, or a {kind, name} natural key. \
                   The whole batch is one transaction."
)]
pub struct ImportParameters {
    #[serde(default)]
    pub on_conflict: ImportConflictParameters,
    #[serde(default)]
    pub nodes: Vec<ImportNodeParameters>,
    #[serde(default)]
    pub edges: Vec<ImportEdgeParameters>,
}

impl From<ImportParameters> for ImportDraft {
    fn from(params: ImportParameters) -> Self {
        ImportDraft {
            on_conflict: params.on_conflict.into(),
            nodes: params.nodes.into_iter().map(Into::into).collect(),
            edges: params.edges.into_iter().map(Into::into).collect(),
        }
    }
}

fn import_status(status: ImportStatus) -> String {
    match status {
        ImportStatus::Created => "created",
        ImportStatus::Updated => "updated",
        ImportStatus::Unchanged => "unchanged",
        ImportStatus::Skipped => "skipped",
        ImportStatus::Existing => "existing",
    }
    .to_string()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ImportNodeResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub id: i64,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ImportEdgeResponse {
    pub source: i64,
    pub destination: i64,
    pub edge_type: String,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ImportReportResponse {
    pub nodes: Vec<ImportNodeResponse>,
    pub edges: Vec<ImportEdgeResponse>,
    pub nodes_created: usize,
    pub nodes_updated: usize,
    pub nodes_unchanged: usize,
    pub nodes_skipped: usize,
    pub edges_created: usize,
    pub edges_existing: usize,
}

impl From<ImportReport> for ImportReportResponse {
    fn from(report: ImportReport) -> Self {
        ImportReportResponse {
            nodes_created: report.nodes_created(),
            nodes_updated: report.nodes_updated(),
            nodes_unchanged: report.nodes_unchanged(),
            nodes_skipped: report.nodes_skipped(),
            edges_created: report.edges_created(),
            edges_existing: report.edges_existing(),
            nodes: report
                .nodes
                .into_iter()
                .map(|node| ImportNodeResponse {
                    reference: node.reference,
                    id: node.id,
                    status: import_status(node.status),
                })
                .collect(),
            edges: report
                .edges
                .into_iter()
                .map(|edge| ImportEdgeResponse {
                    source: edge.source,
                    destination: edge.destination,
                    edge_type: edge.edge_type,
                    status: import_status(edge.status),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParameters {
    pub q: Option<String>,
    pub tags: Option<String>,
    pub properties: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl SearchParameters {
    pub fn tag_list(&self) -> Vec<String> {
        self.tags
            .as_deref()
            .map(|tags| tags.split(',').map(str::to_string).collect())
            .unwrap_or_default()
    }

    pub fn property_filter(&self) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        parse_property_filter(self.properties.as_deref())
    }
}

fn parse_property_filter(
    raw: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let Some(raw) = raw else {
        return Ok(serde_json::Map::new());
    };
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| format!("properties must be a JSON object: {error}"))?;
    match value {
        serde_json::Value::Object(map) => {
            for key in map.keys() {
                if key.is_empty()
                    || key.chars().count() > 128
                    || key.chars().any(char::is_control)
                    || key.contains('"')
                {
                    return Err(format!("invalid property filter key '{key}'"));
                }
            }
            Ok(map)
        }
        _ => Err("properties must be a JSON object".to_string()),
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchHitResponse {
    pub node: NodeResponse,
    pub score: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    pub items: Vec<SearchHitResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TagUsageResponse {
    pub tag: String,
    pub node_count: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReplaceTagsParameters {
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A task created under a parent. Supplying id upserts that node and \
                   replaces its name, description and content; omitted tags leave existing \
                   tags unchanged, an empty list clears them."
)]
pub struct CreateTaskParameters {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub content: String,
    pub parent: Option<i64>,
    #[serde(default)]
    pub depends_on: Vec<i64>,
    #[serde(default)]
    pub affects: Vec<i64>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

impl From<CreateTaskParameters> for TaskDraft {
    fn from(params: CreateTaskParameters) -> Self {
        TaskDraft {
            id: params.id.unwrap_or(0),
            name: params.name,
            description: params.description,
            content: params.content,
            parent: params.parent,
            depends_on: params.depends_on,
            affects: params.affects,
            properties: params.properties,
            tags: params.tags,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePlanParameters {
    pub goal: Option<CreatePlanGoalParameters>,
    pub goal_id: Option<i64>,
    pub anchor: Option<PlanAnchorParameters>,
    #[serde(default)]
    pub tasks: Vec<CreatePlanTaskParameters>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PlanAnchorParameters {
    pub node_id: i64,
    pub edge_type: String,
}

impl From<PlanAnchorParameters> for PlanAnchorDraft {
    fn from(params: PlanAnchorParameters) -> Self {
        PlanAnchorDraft {
            node_id: params.node_id,
            edge_type: params.edge_type,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A plan goal. Omitted tags leave existing tags unchanged on an id upsert; \
                   an empty list clears them."
)]
pub struct CreatePlanGoalParameters {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A plan task. Supplying id upserts that node and replaces its name, \
                   description and content; omitted tags leave existing tags unchanged, an \
                   empty list clears them."
)]
pub struct CreatePlanTaskParameters {
    pub id: Option<i64>,
    pub key: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub affects: Vec<i64>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

impl From<CreatePlanParameters> for PlanDraft {
    fn from(params: CreatePlanParameters) -> Self {
        PlanDraft {
            goal: params.goal.map(Into::into),
            goal_id: params.goal_id,
            anchor: params.anchor.map(Into::into),
            tasks: params.tasks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<CreatePlanGoalParameters> for PlanGoalDraft {
    fn from(params: CreatePlanGoalParameters) -> Self {
        PlanGoalDraft {
            id: params.id.unwrap_or(0),
            name: params.name,
            description: params.description,
            content: params.content,
            properties: params.properties,
            tags: params.tags,
        }
    }
}

impl From<CreatePlanTaskParameters> for PlanTaskDraft {
    fn from(params: CreatePlanTaskParameters) -> Self {
        PlanTaskDraft {
            id: params.id.unwrap_or(0),
            key: params.key,
            name: params.name,
            description: params.description,
            content: params.content,
            parent: params.parent,
            depends_on: params.depends_on,
            affects: params.affects,
            properties: params.properties,
            tags: params.tags,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanTaskResponse {
    pub key: String,
    pub node: NodeResponse,
}

impl From<PlanTaskModel> for PlanTaskResponse {
    fn from(task: PlanTaskModel) -> Self {
        PlanTaskResponse {
            key: task.key,
            node: task.node.into(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<NodeResponse>,
    pub tasks: Vec<PlanTaskResponse>,
}

impl From<PlanModel> for PlanResponse {
    fn from(plan: PlanModel) -> Self {
        PlanResponse {
            goal: plan.goal.map(Into::into),
            tasks: plan.tasks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(
    description = "Partial node update. Only name, description, content and tags are \
                   patchable; kind is immutable and unknown fields are rejected. Omitted \
                   fields are left unchanged, and an empty tag list clears tags."
)]
pub struct UpdateNodeParameters {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub properties: Option<serde_json::Value>,
}

impl From<UpdateNodeParameters> for NodePatch {
    fn from(params: UpdateNodeParameters) -> Self {
        NodePatch {
            name: params.name,
            description: params.description,
            content: params.content,
            tags: params.tags,
            properties: params.properties,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "A task patch. Omitted tags leave the current tags unchanged; an empty \
                   list clears them."
)]
pub struct UpdateTaskParameters {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

impl From<UpdateTaskParameters> for TaskPatch {
    fn from(params: UpdateTaskParameters) -> Self {
        TaskPatch {
            name: params.name,
            description: params.description,
            content: params.content,
            properties: params.properties,
            tags: params.tags,
        }
    }
}
