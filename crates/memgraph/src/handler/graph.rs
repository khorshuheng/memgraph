use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    dto::{
        audit::{SessionHeader, SessionId},
        graph::{
            CreateEdgeParameters, CreateEdgeTypeParameters, CreateNodeKindParameters,
            CreateNodeParameters, CreatePlanParameters, CreateTaskParameters, DeleteEdgeParameters,
            EdgeTypeResponse, NeighborParameters, NeighborResponse, NodeKindResponse, NodeResponse,
            PlanResponse, SearchHitResponse, SearchParameters, SearchResponse,
            SearchSegmentParameters, SegmentResponse, UpdateTaskParameters,
        },
        health::GraphHealthResponse,
        wrapper::{ApiError, ApiList, ApiSuccess},
    },
    model::{AccessAction, Edge, EdgeType, Node, NodeKind, PlanDraft, TaskDraft, TaskPatch},
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
    params(SessionHeader),
    responses((status = 201, description = "Node created", body = NodeResponse))
)]
pub async fn create_node(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Json(params): Json<CreateNodeParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let node: Node = params.into();
    let created = state.graph_service.upsert_node(&node).await?;
    state
        .audit_service
        .record(&session_id, &[created.id], AccessAction::Write)
        .await;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: created.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/tasks",
    request_body = CreateTaskParameters,
    params(SessionHeader),
    responses((status = 201, description = "Task created", body = NodeResponse))
)]
pub async fn create_task(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Json(params): Json<CreateTaskParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let draft: TaskDraft = params.into();
    let created = state.graph_service.create_task(&draft).await?;
    let mut accessed = vec![created.id];
    accessed.extend(draft.parent);
    accessed.extend(draft.depends_on.iter().copied());
    accessed.extend(draft.affects.iter().copied());
    state
        .audit_service
        .record(&session_id, &accessed, AccessAction::Write)
        .await;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: created.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/plans",
    request_body = CreatePlanParameters,
    params(SessionHeader),
    responses((status = 201, description = "Plan created", body = PlanResponse))
)]
pub async fn create_plan(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Json(params): Json<CreatePlanParameters>,
) -> Result<ApiSuccess<PlanResponse>, ApiError> {
    let draft: PlanDraft = params.into();
    let plan = state.graph_service.create_plan(&draft).await?;
    let mut accessed: Vec<i64> = plan.goal.iter().map(|goal| goal.id).collect();
    accessed.extend(plan.tasks.iter().map(|task| task.node.id));
    if let Some(goal_id) = draft.goal_id {
        accessed.push(goal_id);
    }
    accessed.extend(
        draft
            .tasks
            .iter()
            .flat_map(|task| task.affects.iter().copied()),
    );
    state
        .audit_service
        .record(&session_id, &accessed, AccessAction::Write)
        .await;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: plan.into(),
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
    patch,
    path = "/tasks/{task_id}",
    request_body = UpdateTaskParameters,
    params(SessionHeader, ("task_id" = i64, Path, description = "Task id")),
    responses((status = 200, description = "Task updated", body = NodeResponse))
)]
pub async fn update_task(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Path(task_id): Path<i64>,
    Json(params): Json<UpdateTaskParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let patch: TaskPatch = params.into();
    let updated = state.graph_service.update_task(task_id, &patch).await?;
    state
        .audit_service
        .record(&session_id, &[updated.id], AccessAction::Write)
        .await;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: updated.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes/{node_id}",
    params(SessionHeader, ("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Node", body = NodeResponse))
)]
pub async fn get_node(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Path(node_id): Path<i64>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let node = state.graph_service.get_node(node_id).await?;
    state
        .audit_service
        .record(&session_id, &[node.id], AccessAction::Read)
        .await;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: node.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    delete,
    path = "/nodes/{node_id}",
    params(SessionHeader, ("node_id" = i64, Path, description = "Node id")),
    responses((status = 204, description = "Node deleted"))
)]
pub async fn delete_node(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Path(node_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .audit_service
        .record(&session_id, &[node_id], AccessAction::Write)
        .await;
    state.graph_service.delete_node(node_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes/{node_id}/neighbors",
    params(SessionHeader, NeighborParameters, ("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Neighbors of a node", body = ApiList<NeighborResponse>))
)]
pub async fn list_neighbors(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Path(node_id): Path<i64>,
    Query(params): Query<NeighborParameters>,
) -> Result<ApiList<NeighborResponse>, ApiError> {
    let neighbors = state
        .graph_service
        .neighbors(
            node_id,
            params.direction.into(),
            params.edge_type.as_deref(),
        )
        .await?;
    let mut accessed = vec![node_id];
    accessed.extend(neighbors.iter().map(|neighbor| neighbor.node.id));
    state
        .audit_service
        .record(&session_id, &accessed, AccessAction::Read)
        .await;
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
    params(SessionHeader),
    responses((status = 204, description = "Edge created"))
)]
pub async fn add_edge(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Json(params): Json<CreateEdgeParameters>,
) -> Result<StatusCode, ApiError> {
    let edge: Edge = params.into();
    state.graph_service.add_edge(&edge).await?;
    state
        .audit_service
        .record(
            &session_id,
            &[edge.source, edge.destination],
            AccessAction::Write,
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    delete,
    path = "/edges",
    params(SessionHeader, DeleteEdgeParameters),
    responses((status = 204, description = "Edge removed"))
)]
pub async fn remove_edge(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Query(params): Query<DeleteEdgeParameters>,
) -> Result<StatusCode, ApiError> {
    let edge: Edge = params.into();
    state.graph_service.remove_edge(&edge).await?;
    state
        .audit_service
        .record(
            &session_id,
            &[edge.source, edge.destination],
            AccessAction::Write,
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/search",
    params(SessionHeader, SearchParameters),
    responses((status = 200, description = "Per-segment full-text search hits", body = SearchResponse))
)]
pub async fn search(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Query(params): Query<SearchParameters>,
) -> Result<ApiSuccess<SearchResponse>, ApiError> {
    let scope_value = params.scope.as_str();
    let scope = params.scope.into();
    let segments = state.graph_service.search(&params.q, scope).await?;
    let accessed: Vec<i64> = segments
        .iter()
        .flat_map(|segment| segment.matches.iter().map(|hit| hit.node.id))
        .collect();
    state
        .audit_service
        .record(&session_id, &accessed, AccessAction::Read)
        .await;
    let summary_chars = state.graph_service.summary_chars();
    let segments = segments
        .into_iter()
        .map(|segment| {
            let hidden = segment.total_matches.saturating_sub(segment.matches.len());
            let probe = segment_probe(
                &segment.segment,
                scope_value,
                segment.total_matches,
                segment.matches.len(),
            );
            SegmentResponse {
                segment: segment.segment,
                matches: segment
                    .matches
                    .into_iter()
                    .map(|hit| SearchHitResponse::new(hit, summary_chars))
                    .collect(),
                total_matches: segment.total_matches,
                hidden,
                excluded_resolved: segment.excluded_resolved,
                probe,
            }
        })
        .collect();
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: SearchResponse { segments },
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/search/segment",
    params(SessionHeader, SearchSegmentParameters),
    responses((status = 200, description = "Additional matches for a single segment", body = ApiList<SearchHitResponse>))
)]
pub async fn search_segment(
    State(state): State<AppState>,
    SessionId(session_id): SessionId,
    Query(params): Query<SearchSegmentParameters>,
) -> Result<ApiList<SearchHitResponse>, ApiError> {
    let limit = params
        .limit
        .unwrap_or_else(|| state.graph_service.default_segment_limit());
    let offset = params.offset.unwrap_or(0);
    let hits = state
        .graph_service
        .search_segment(&params.segment, params.scope.into(), limit, offset)
        .await?;
    let accessed: Vec<i64> = hits.iter().map(|hit| hit.node.id).collect();
    state
        .audit_service
        .record(&session_id, &accessed, AccessAction::Read)
        .await;
    let summary_chars = state.graph_service.summary_chars();
    Ok(ApiList {
        status: StatusCode::OK,
        items: hits
            .into_iter()
            .map(|hit| SearchHitResponse::new(hit, summary_chars))
            .collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/graph-health",
    responses((status = 200, description = "Read-only graph shape report", body = GraphHealthResponse))
)]
pub async fn graph_health(
    State(state): State<AppState>,
) -> Result<ApiSuccess<GraphHealthResponse>, ApiError> {
    let health = state.health_service.report().await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: health.into(),
    })
}

fn segment_probe(segment: &str, scope: &str, total_matches: usize, shown: usize) -> Option<String> {
    let hidden = total_matches.saturating_sub(shown);
    (hidden > 0).then(|| {
        format!(
            "/api/search/segment?segment={}&scope={}&limit={}&offset={}",
            percent_encode(segment),
            scope,
            total_matches,
            shown
        )
    })
}

fn percent_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_preserves_scope_and_counts() {
        assert_eq!(
            segment_probe("a b", "all", 7, 5).as_deref(),
            Some("/api/search/segment?segment=a%20b&scope=all&limit=7&offset=5")
        );
        assert!(segment_probe("a", "active", 5, 5).is_none());
    }
}
