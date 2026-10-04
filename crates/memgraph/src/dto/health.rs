use serde::Serialize;
use utoipa::ToSchema;

use crate::model::{
    DuplicateGroup as DuplicateGroupModel, EdgeTypeUsage as EdgeTypeUsageModel,
    GraphHealth as GraphHealthModel, IsolatedNode as IsolatedNodeModel,
    KindUsage as KindUsageModel,
};

#[derive(Serialize, ToSchema)]
pub enum HealthCheckStatus {
    Pass,
    #[allow(dead_code)]
    Fail,
}

#[derive(Serialize, ToSchema)]
pub struct HealthCheckResponse {
    pub result: HealthCheckStatus,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct KindUsageResponse {
    pub kind_id: i64,
    pub kind: String,
    pub node_count: i64,
    pub dead: bool,
}

impl From<KindUsageModel> for KindUsageResponse {
    fn from(usage: KindUsageModel) -> Self {
        KindUsageResponse {
            dead: usage.node_count == 0,
            kind_id: usage.kind_id,
            kind: usage.kind,
            node_count: usage.node_count,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EdgeTypeUsageResponse {
    pub edge_type_id: i64,
    pub edge_type: String,
    pub edge_count: i64,
    pub underused: bool,
}

impl From<EdgeTypeUsageModel> for EdgeTypeUsageResponse {
    fn from(usage: EdgeTypeUsageModel) -> Self {
        EdgeTypeUsageResponse {
            underused: usage.edge_count <= 1,
            edge_type_id: usage.edge_type_id,
            edge_type: usage.edge_type,
            edge_count: usage.edge_count,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DuplicateGroupResponse {
    pub kind_id: i64,
    pub kind: String,
    pub name: String,
    pub node_ids: Vec<i64>,
}

impl From<DuplicateGroupModel> for DuplicateGroupResponse {
    fn from(group: DuplicateGroupModel) -> Self {
        DuplicateGroupResponse {
            kind_id: group.kind_id,
            kind: group.kind,
            name: group.name,
            node_ids: group.node_ids,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IsolatedNodeResponse {
    pub id: i64,
    pub kind: String,
    pub name: String,
}

impl From<IsolatedNodeModel> for IsolatedNodeResponse {
    fn from(node: IsolatedNodeModel) -> Self {
        IsolatedNodeResponse {
            id: node.id,
            kind: node.kind,
            name: node.name,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GraphHealthResponse {
    pub node_count: i64,
    pub edge_count: i64,
    pub kind_count: i64,
    pub edge_type_count: i64,
    pub kinds: Vec<KindUsageResponse>,
    pub edge_types: Vec<EdgeTypeUsageResponse>,
    pub duplicate_groups: Vec<DuplicateGroupResponse>,
    pub isolated_node_count: i64,
    pub isolated_nodes: Vec<IsolatedNodeResponse>,
}

impl From<GraphHealthModel> for GraphHealthResponse {
    fn from(health: GraphHealthModel) -> Self {
        GraphHealthResponse {
            node_count: health.node_count,
            edge_count: health.edge_count,
            kind_count: health.kinds.len() as i64,
            edge_type_count: health.edge_types.len() as i64,
            kinds: health.kinds.into_iter().map(Into::into).collect(),
            edge_types: health.edge_types.into_iter().map(Into::into).collect(),
            duplicate_groups: health.duplicates.into_iter().map(Into::into).collect(),
            isolated_node_count: health.isolated_count,
            isolated_nodes: health.isolated.into_iter().map(Into::into).collect(),
        }
    }
}
