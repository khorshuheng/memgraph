use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    dto::{
        graph::{
            CreateEdgeParameters, CreateEdgeTypeParameters, CreateNodeKindParameters,
            CreateNodeParameters, DeleteEdgeParameters, EdgeTypeResponse, NeighborResponse,
            NodeKindResponse, NodeResponse, SearchHitResponse, SearchParameters,
        },
        wrapper::{ApiError, ApiList, ApiSuccess},
    },
    model::{Edge, EdgeType, Node, NodeKind},
    state::AppState,
};

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/node-kinds",
    request_body = CreateNodeKindParameters,
    responses((status = 201, description = "Node kind created", body = NodeKindResponse))
)]
pub async fn create_node_kind(
    State(state): State<AppState>,
    Json(params): Json<CreateNodeKindParameters>,
) -> Result<ApiSuccess<NodeKindResponse>, ApiError> {
    let node_kind: NodeKind = params.into();
    let created = state.graph_service.create_node_kind(&node_kind).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: created.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/node-kinds",
    responses((status = 200, description = "List node kinds", body = ApiList<NodeKindResponse>))
)]
pub async fn list_node_kinds(
    State(state): State<AppState>,
) -> Result<ApiList<NodeKindResponse>, ApiError> {
    let node_kinds = state.graph_service.list_node_kinds().await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: node_kinds.into_iter().map(Into::into).collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/edge-types",
    request_body = CreateEdgeTypeParameters,
    responses((status = 201, description = "Edge type created", body = EdgeTypeResponse))
)]
pub async fn create_edge_type(
    State(state): State<AppState>,
    Json(params): Json<CreateEdgeTypeParameters>,
) -> Result<ApiSuccess<EdgeTypeResponse>, ApiError> {
    let edge_type: EdgeType = params.into();
    let created = state.graph_service.create_edge_type(&edge_type).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: created.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/edge-types",
    responses((status = 200, description = "List edge types", body = ApiList<EdgeTypeResponse>))
)]
pub async fn list_edge_types(
    State(state): State<AppState>,
) -> Result<ApiList<EdgeTypeResponse>, ApiError> {
    let edge_types = state.graph_service.list_edge_types().await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: edge_types.into_iter().map(Into::into).collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/nodes",
    request_body = CreateNodeParameters,
    responses((status = 201, description = "Node created", body = NodeResponse))
)]
pub async fn create_node(
    State(state): State<AppState>,
    Json(params): Json<CreateNodeParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let node: Node = params.into();
    let created = state.graph_service.upsert_node(&node).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: created.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes",
    responses((status = 200, description = "List nodes", body = ApiList<NodeResponse>))
)]
pub async fn list_nodes(State(state): State<AppState>) -> Result<ApiList<NodeResponse>, ApiError> {
    let nodes = state.graph_service.list_nodes().await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: nodes.into_iter().map(Into::into).collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes/{node_id}",
    params(("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Node", body = NodeResponse))
)]
pub async fn get_node(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let node = state.graph_service.get_node(node_id).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: node.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    delete,
    path = "/nodes/{node_id}",
    params(("node_id" = i64, Path, description = "Node id")),
    responses((status = 204, description = "Node deleted"))
)]
pub async fn delete_node(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.graph_service.delete_node(node_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes/{node_id}/neighbors",
    params(("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Outgoing neighbors", body = ApiList<NeighborResponse>))
)]
pub async fn list_neighbors(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> Result<ApiList<NeighborResponse>, ApiError> {
    let neighbors = state.graph_service.neighbors(node_id).await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: neighbors.into_iter().map(Into::into).collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/edges",
    request_body = CreateEdgeParameters,
    responses((status = 204, description = "Edge created"))
)]
pub async fn add_edge(
    State(state): State<AppState>,
    Json(params): Json<CreateEdgeParameters>,
) -> Result<StatusCode, ApiError> {
    let edge: Edge = params.into();
    state.graph_service.add_edge(&edge).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    delete,
    path = "/edges",
    params(DeleteEdgeParameters),
    responses((status = 204, description = "Edge removed"))
)]
pub async fn remove_edge(
    State(state): State<AppState>,
    Query(params): Query<DeleteEdgeParameters>,
) -> Result<StatusCode, ApiError> {
    let edge: Edge = params.into();
    state.graph_service.remove_edge(&edge).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/search",
    params(SearchParameters),
    responses((status = 200, description = "Full-text search hits", body = ApiList<SearchHitResponse>))
)]
pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParameters>,
) -> Result<ApiList<SearchHitResponse>, ApiError> {
    let hits = state.graph_service.search(&params.q).await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: hits.into_iter().map(Into::into).collect(),
    })
}
