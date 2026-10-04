use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::model::{
    Edge as EdgeModel, EdgeType as EdgeTypeModel, Neighbor as NeighborModel,
    NeighborDirection as NeighborDirectionModel, Node as NodeModel, NodeFilter,
    NodeKind as NodeKindModel, Plan as PlanModel, PlanAnchorDraft, PlanDraft, PlanGoalDraft,
    PlanTask as PlanTaskModel, PlanTaskDraft, ReachFilter,
    RelationDirection as RelationDirectionModel, RelationSummary as RelationSummaryModel,
    SearchHit as SearchHitModel, SearchScope as SearchScopeModel, TaskDraft, TaskPatch,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateNodeKindParameters {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NodeKindResponse {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub updated_at: String,
}

impl From<CreateNodeKindParameters> for NodeKindModel {
    fn from(params: CreateNodeKindParameters) -> Self {
        NodeKindModel {
            id: 0,
            name: params.name,
            description: params.description,
            updated_at: String::new(),
        }
    }
}

impl From<NodeKindModel> for NodeKindResponse {
    fn from(node_kind: NodeKindModel) -> Self {
        NodeKindResponse {
            id: node_kind.id,
            name: node_kind.name,
            description: node_kind.description,
            updated_at: node_kind.updated_at,
        }
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
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NodeResponse {
    pub id: i64,
    pub kind_id: i64,
    pub name: String,
    pub description: String,
    pub content: String,
    pub updated_at: String,
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

impl From<NodeModel> for NodeResponse {
    fn from(node: NodeModel) -> Self {
        NodeResponse {
            id: node.id,
            kind_id: node.kind_id,
            name: node.name,
            description: node.description,
            content: node.content,
            updated_at: node.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEdgeParameters {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeResponse {
    pub source: i64,
    pub destination: i64,
    pub edge_type_id: i64,
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

impl From<NeighborModel> for NeighborResponse {
    fn from(neighbor: NeighborModel) -> Self {
        NeighborResponse {
            direction: match neighbor.direction {
                RelationDirectionModel::Outgoing => "outgoing".to_string(),
                RelationDirectionModel::Ingoing => "incoming".to_string(),
            },
            edge: neighbor.edge.into(),
            node: neighbor.node.into(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RelationSummaryResponse {
    pub direction: String,
    pub edge_type_id: i64,
    pub edge_type: String,
    pub count: i64,
}

impl From<RelationSummaryModel> for RelationSummaryResponse {
    fn from(summary: RelationSummaryModel) -> Self {
        RelationSummaryResponse {
            direction: match summary.direction {
                RelationDirectionModel::Outgoing => "outgoing".to_string(),
                RelationDirectionModel::Ingoing => "incoming".to_string(),
            },
            edge_type_id: summary.edge_type_id,
            edge_type: summary.edge_type,
            count: summary.count,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchHitResponse {
    pub id: i64,
    pub kind_id: i64,
    pub name: String,
    pub summary: String,
    pub confidence: f64,
    pub matched_terms: Vec<String>,
    pub relations: Vec<RelationSummaryResponse>,
    pub resolved: bool,
    pub access_id: Option<i64>,
}

impl SearchHitResponse {
    pub fn new(hit: SearchHitModel, summary_chars: usize, access_id: Option<i64>) -> Self {
        SearchHitResponse {
            id: hit.node.id,
            kind_id: hit.node.kind_id,
            name: hit.node.name,
            summary: truncate_summary(&hit.node.description, summary_chars),
            confidence: hit.confidence,
            matched_terms: hit.matched_terms,
            relations: hit.relations.into_iter().map(Into::into).collect(),
            resolved: hit.resolved,
            access_id,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SegmentResponse {
    pub segment: String,
    pub matches: Vec<SearchHitResponse>,
    pub total_matches: usize,
    pub hidden: usize,
    pub excluded_resolved: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    pub segments: Vec<SegmentResponse>,
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    let truncated: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{truncated}…")
    } else {
        truncated
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

impl SearchScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchScope::Active => "active",
            SearchScope::All => "all",
            SearchScope::Resolved => "resolved",
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParameters {
    pub q: String,
    #[serde(default)]
    pub scope: SearchScope,
    pub within: Option<i64>,
    pub via: Option<String>,
    pub descend: Option<String>,
}

impl SearchParameters {
    pub fn reach(&self) -> ReachFilter {
        ReachFilter {
            within: self.within,
            via: self.via.clone(),
            descend: self.descend.clone(),
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchSegmentParameters {
    pub segment: String,
    #[serde(default)]
    pub scope: SearchScope,
    pub within: Option<i64>,
    pub via: Option<String>,
    pub descend: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl SearchSegmentParameters {
    pub fn reach(&self) -> ReachFilter {
        ReachFilter {
            within: self.within,
            via: self.via.clone(),
            descend: self.descend.clone(),
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
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NodeAccessFeedbackParameters {
    pub relevant: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
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
pub struct CreatePlanGoalParameters {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Deserialize, ToSchema)]
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
pub struct UpdateTaskParameters {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
}

impl From<UpdateTaskParameters> for TaskPatch {
    fn from(params: UpdateTaskParameters) -> Self {
        TaskPatch {
            name: params.name,
            description: params.description,
            content: params.content,
        }
    }
}
