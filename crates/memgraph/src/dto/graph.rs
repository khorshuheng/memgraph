use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::model::{
    Edge as EdgeModel, EdgeType as EdgeTypeModel, Node as NodeModel, NodeKind as NodeKindModel,
    RelationDirection as RelationDirectionModel, RelationSummary as RelationSummaryModel,
    SearchHit as SearchHitModel,
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
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeTypeResponse {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub updated_at: String,
}

impl From<CreateEdgeTypeParameters> for EdgeTypeModel {
    fn from(params: CreateEdgeTypeParameters) -> Self {
        EdgeTypeModel {
            id: 0,
            name: params.name,
            description: params.description,
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

#[derive(Debug, Serialize, ToSchema)]
pub struct NeighborResponse {
    pub edge: EdgeResponse,
    pub node: NodeResponse,
}

impl From<(EdgeModel, NodeModel)> for NeighborResponse {
    fn from((edge, node): (EdgeModel, NodeModel)) -> Self {
        NeighborResponse {
            edge: edge.into(),
            node: node.into(),
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
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SegmentResponse {
    pub segment: String,
    pub matches: Vec<SearchHitResponse>,
    pub total_matches: usize,
    pub hidden: usize,
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

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParameters {
    pub q: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchSegmentParameters {
    pub segment: String,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
