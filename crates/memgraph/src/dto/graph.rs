use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::model::{
    Edge as EdgeModel, EdgeType as EdgeTypeModel, Node as NodeModel, NodeKind as NodeKindModel,
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
}

impl From<CreateNodeKindParameters> for NodeKindModel {
    fn from(params: CreateNodeKindParameters) -> Self {
        NodeKindModel {
            id: 0,
            name: params.name,
            description: params.description,
        }
    }
}

impl From<NodeKindModel> for NodeKindResponse {
    fn from(node_kind: NodeKindModel) -> Self {
        NodeKindResponse {
            id: node_kind.id,
            name: node_kind.name,
            description: node_kind.description,
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
}

impl From<CreateEdgeTypeParameters> for EdgeTypeModel {
    fn from(params: CreateEdgeTypeParameters) -> Self {
        EdgeTypeModel {
            id: 0,
            name: params.name,
            description: params.description,
        }
    }
}

impl From<EdgeTypeModel> for EdgeTypeResponse {
    fn from(edge_type: EdgeTypeModel) -> Self {
        EdgeTypeResponse {
            id: edge_type.id,
            name: edge_type.name,
            description: edge_type.description,
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
}

impl From<CreateNodeParameters> for NodeModel {
    fn from(params: CreateNodeParameters) -> Self {
        NodeModel {
            id: 0,
            kind_id: params.kind_id,
            name: params.name,
            description: params.description,
            content: params.content,
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
}

impl From<CreateEdgeParameters> for EdgeModel {
    fn from(params: CreateEdgeParameters) -> Self {
        EdgeModel {
            source: params.source,
            destination: params.destination,
            edge_type_id: params.edge_type_id,
        }
    }
}

impl From<EdgeModel> for EdgeResponse {
    fn from(edge: EdgeModel) -> Self {
        EdgeResponse {
            source: edge.source,
            destination: edge.destination,
            edge_type_id: edge.edge_type_id,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
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
pub struct SearchHitResponse {
    pub node: NodeResponse,
    pub score: f64,
}

impl From<SearchHitModel> for SearchHitResponse {
    fn from(hit: SearchHitModel) -> Self {
        SearchHitResponse {
            node: hit.node.into(),
            score: hit.score,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SearchParameters {
    pub q: String,
}
