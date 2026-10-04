use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::model::{
    Edge as EdgeModel, EdgeType as EdgeTypeModel, Neighbor as NeighborModel,
    NeighborDirection as NeighborDirectionModel, Node as NodeModel, NodeKind as NodeKindModel,
    Plan as PlanModel, PlanDraft, PlanGoalDraft, PlanTask as PlanTaskModel, PlanTaskDraft,
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
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeTypeResponse {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub resolves: bool,
    pub updated_at: String,
}

impl From<CreateEdgeTypeParameters> for EdgeTypeModel {
    fn from(params: CreateEdgeTypeParameters) -> Self {
        EdgeTypeModel {
            id: 0,
            name: params.name,
            description: params.description,
            resolves: params.resolves,
            updated_at: String::new(),
        }
    }
}

impl From<EdgeTypeModel> for EdgeTypeResponse {
    fn from(edge_type: EdgeTypeModel) -> Self {
        EdgeTypeResponse {
            id: edge_type.id,
            name: edge_type.name,
            description: edge_type.description,
            resolves: edge_type.resolves,
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
}

impl SearchHitResponse {
    pub fn new(hit: SearchHitModel, summary_chars: usize) -> Self {
        SearchHitResponse {
            id: hit.node.id,
            kind_id: hit.node.kind_id,
            name: hit.node.name,
            summary: truncate_summary(&hit.node.description, summary_chars),
            confidence: hit.confidence,
            matched_terms: hit.matched_terms,
            relations: hit.relations.into_iter().map(Into::into).collect(),
            resolved: hit.resolved,
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
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchSegmentParameters {
    pub segment: String,
    #[serde(default)]
    pub scope: SearchScope,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
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
    #[serde(default)]
    pub tasks: Vec<CreatePlanTaskParameters>,
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
