use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    dto::{
        graph::{
            ApplyPackParameters, CreateEdgeParameters, CreateEdgeTypeParameters,
            CreateNodeKindParameters, CreateNodeParameters, CreatePlanParameters,
            CreateTaskParameters, DeleteEdgeParameters, EdgeTypeResponse, ImportParameters,
            ImportReportResponse, ListNodesParameters, NeighborParameters, NeighborResponse,
            NodeKindResponse, NodeResponse, PackReportResponse, PlanResponse,
            ReplaceTagsParameters, SearchHitResponse, SearchParameters, SearchResponse,
            TagUsageResponse, UpdateNodeParameters, UpdateTaskParameters,
        },
        health::GraphHealthResponse,
        wrapper::{ApiError, ApiList, ApiSuccess},
    },
    model::{
        Edge, EdgeType, ImportDraft, Node, NodeFilter, NodeKind, NodePatch, PackDraft, PlanDraft,
        TaskDraft, TaskPatch,
    },
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
    path = "/pack",
    request_body = ApplyPackParameters,
    responses((status = 200, description = "Vocabulary pack applied", body = PackReportResponse))
)]
pub async fn apply_pack(
    State(state): State<AppState>,
    Json(params): Json<ApplyPackParameters>,
) -> Result<ApiSuccess<PackReportResponse>, ApiError> {
    let pack: PackDraft = params.into();
    let report = state.graph_service.apply_pack(&pack).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: report.into(),
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
    let tags = params.tags.clone();
    let properties = params.properties.clone();
    let node: Node = params.into();
    let (created, tags) = state
        .graph_service
        .create_node_with_properties(&node, &tags, properties.as_ref())
        .await?;
    let properties = state.graph_service.node_properties(created.id).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: NodeResponse::with_tags_and_properties(created, tags, properties),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/tasks",
    request_body = CreateTaskParameters,
    responses((status = 201, description = "Task created", body = NodeResponse))
)]
pub async fn create_task(
    State(state): State<AppState>,
    Json(params): Json<CreateTaskParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let draft: TaskDraft = params.into();
    let created = state.graph_service.create_task(&draft).await?;
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
    responses((status = 201, description = "Plan created", body = PlanResponse))
)]
pub async fn create_plan(
    State(state): State<AppState>,
    Json(params): Json<CreatePlanParameters>,
) -> Result<ApiSuccess<PlanResponse>, ApiError> {
    let draft: PlanDraft = params.into();
    let plan = state.graph_service.create_plan(&draft).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data: plan.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/graph/import",
    request_body = ImportParameters,
    responses((status = 200, description = "Batch import applied", body = ImportReportResponse))
)]
pub async fn import_graph(
    State(state): State<AppState>,
    Json(params): Json<ImportParameters>,
) -> Result<ApiSuccess<ImportReportResponse>, ApiError> {
    let draft: ImportDraft = params.into();
    let report = state.graph_service.import_graph(&draft).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: report.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes",
    params(ListNodesParameters),
    responses((status = 200, description = "List nodes", body = ApiList<NodeResponse>))
)]
pub async fn list_nodes(
    State(state): State<AppState>,
    Query(params): Query<ListNodesParameters>,
) -> Result<ApiList<NodeResponse>, ApiError> {
    let properties = params.property_filter().map_err(|message| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        message,
    })?;
    let mut filter: NodeFilter = params.into();
    filter.properties = properties;
    let nodes = state.graph_service.list_nodes(&filter).await?;
    let ids: Vec<i64> = nodes.iter().map(|node| node.id).collect();
    let mut tags = state.graph_service.node_tags_for_ids(&ids).await?;
    let mut properties = state.graph_service.node_properties_for_ids(&ids).await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: nodes
            .into_iter()
            .map(|node| {
                let node_tags = tags.remove(&node.id).unwrap_or_default();
                let node_properties = properties
                    .remove(&node.id)
                    .unwrap_or_else(|| serde_json::json!({}));
                NodeResponse::with_tags_and_properties(node, node_tags, node_properties)
            })
            .collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    patch,
    path = "/tasks/{task_id}",
    request_body = UpdateTaskParameters,
    params(("task_id" = i64, Path, description = "Task id")),
    responses((status = 200, description = "Task updated", body = NodeResponse))
)]
pub async fn update_task(
    State(state): State<AppState>,
    Path(task_id): Path<i64>,
    Json(params): Json<UpdateTaskParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let patch: TaskPatch = params.into();
    let updated = state.graph_service.update_task(task_id, &patch).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: updated.into(),
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
    let tags = state.graph_service.node_tags(node_id).await?;
    let properties = state.graph_service.node_properties(node_id).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: NodeResponse::with_tags_and_properties(node, tags, properties),
    })
}

#[utoipa::path(
    tag = "Graph",
    patch,
    path = "/nodes/{node_id}",
    request_body = UpdateNodeParameters,
    params(("node_id" = i64, Path, description = "Node id")),
    responses(
        (status = 200, description = "Node updated", body = NodeResponse),
        (status = 404, description = "Node not found"),
        (status = 409, description = "Another node already uses that name"),
        (status = 422, description = "Empty or invalid patch")
    )
)]
pub async fn update_node(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
    Json(params): Json<UpdateNodeParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let patch: NodePatch = params.into();
    let (node, tags) = state.graph_service.update_node(node_id, &patch).await?;
    let properties = state.graph_service.node_properties(node_id).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: NodeResponse::with_tags_and_properties(node, tags, properties),
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
    path = "/nodes/{node_id}/tags",
    params(("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Tags attached to a node", body = ApiList<String>))
)]
pub async fn get_node_tags(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> Result<ApiList<String>, ApiError> {
    let tags = state.graph_service.node_tags(node_id).await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: tags,
    })
}

#[utoipa::path(
    tag = "Graph",
    put,
    path = "/nodes/{node_id}/tags",
    request_body = ReplaceTagsParameters,
    params(("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Tags replaced", body = ApiList<String>))
)]
pub async fn replace_node_tags(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
    Json(params): Json<ReplaceTagsParameters>,
) -> Result<ApiList<String>, ApiError> {
    let tags = state
        .graph_service
        .replace_node_tags(node_id, &params.tags)
        .await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: tags,
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/tags",
    responses((status = 200, description = "Known tags with node counts", body = ApiList<TagUsageResponse>))
)]
pub async fn list_tags(
    State(state): State<AppState>,
) -> Result<ApiList<TagUsageResponse>, ApiError> {
    let tags = state.graph_service.list_tags().await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: tags
            .into_iter()
            .map(|usage| TagUsageResponse {
                tag: usage.tag,
                node_count: usage.node_count,
            })
            .collect(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/nodes/{node_id}/neighbors",
    params(NeighborParameters, ("node_id" = i64, Path, description = "Node id")),
    responses((status = 200, description = "Neighbors of a node", body = ApiList<NeighborResponse>))
)]
pub async fn list_neighbors(
    State(state): State<AppState>,
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
    let ids: Vec<i64> = neighbors.iter().map(|neighbor| neighbor.node.id).collect();
    let mut tags = state.graph_service.node_tags_for_ids(&ids).await?;
    let mut properties = state.graph_service.node_properties_for_ids(&ids).await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: neighbors
            .into_iter()
            .map(|neighbor| {
                let node_tags = tags.remove(&neighbor.node.id).unwrap_or_default();
                let node_properties = properties
                    .remove(&neighbor.node.id)
                    .unwrap_or_else(|| serde_json::json!({}));
                NeighborResponse::with_tags_and_properties(neighbor, node_tags, node_properties)
            })
            .collect(),
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
    let properties = params.properties.clone();
    let edge: Edge = params.into();
    state
        .graph_service
        .add_edge_with_properties(&edge, properties.as_ref())
        .await?;
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
    responses((status = 200, description = "Full-text and tag search", body = SearchResponse))
)]
pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParameters>,
) -> Result<ApiSuccess<SearchResponse>, ApiError> {
    let tags = params.tag_list();
    let properties = params.property_filter().map_err(|message| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        message,
    })?;
    let page = state
        .graph_service
        .search(
            params.q.as_deref(),
            &tags,
            &properties,
            params.limit,
            params.offset,
        )
        .await?;
    let hit_ids: Vec<i64> = page.hits.iter().map(|hit| hit.node.id).collect();
    let mut properties = state
        .graph_service
        .node_properties_for_ids(&hit_ids)
        .await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: SearchResponse {
            items: page
                .hits
                .into_iter()
                .map(|hit| {
                    let node_properties = properties
                        .remove(&hit.node.id)
                        .unwrap_or_else(|| serde_json::json!({}));
                    SearchHitResponse {
                        node: NodeResponse::with_tags_and_properties(
                            hit.node,
                            hit.tags,
                            node_properties,
                        ),
                        score: hit.score,
                    }
                })
                .collect(),
            total: page.total,
            limit: page.limit,
            offset: page.offset,
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, DatabaseConfig, SearchConfig, ServerConfig};
    use crate::dto::graph::{
        ImportConflictParameters, ImportNodeParameters, NeighborDirection, PackEdgeTypeParameters,
        PackNodeKindParameters,
    };

    async fn test_state() -> AppState {
        AppState::new(&AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 0,
            },
            database: DatabaseConfig {
                path: ":memory:".to_string(),
                max_connections: 1,
            },
            search: SearchConfig::default(),
        })
        .await
    }

    async fn seed_kind(state: &AppState, name: &str) -> i64 {
        state
            .graph_service
            .create_node_kind(&NodeKind {
                id: 0,
                version: 0,
                name: name.to_string(),
                description: String::new(),
                updated_at: String::new(),
            })
            .await
            .expect("create node kind")
            .id
    }

    async fn new_node(
        state: &AppState,
        kind_id: i64,
        name: &str,
        description: &str,
        content: &str,
        tags: &[&str],
    ) -> NodeResponse {
        create_node(
            State(state.clone()),
            Json(CreateNodeParameters {
                kind_id,
                name: name.to_string(),
                description: description.to_string(),
                content: content.to_string(),
                tags: tags.iter().map(|tag| tag.to_string()).collect(),
                properties: None,
            }),
        )
        .await
        .expect("create node")
        .data
    }

    async fn search_handler(
        state: &AppState,
        params: SearchParameters,
    ) -> Result<ApiSuccess<SearchResponse>, ApiError> {
        super::search(State(state.clone()), Query(params)).await
    }

    #[tokio::test]
    async fn create_node_normalizes_and_returns_tags() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let created = new_node(
            &state,
            kind,
            "use sqlite",
            "single file store",
            "content",
            &[" Repo:X ", "repo:x", "TOPIC:storage"],
        )
        .await;
        assert_eq!(created.tags, vec!["repo:x", "topic:storage"]);
        assert_eq!(created.name, "use sqlite");

        let fetched = get_node(State(state.clone()), Path(created.id))
            .await
            .expect("get node")
            .data;
        assert_eq!(fetched.tags, vec!["repo:x", "topic:storage"]);
    }

    #[tokio::test]
    async fn list_nodes_and_tags_directory_expose_tags() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        new_node(&state, kind, "a", "first", "", &["alpha"]).await;
        new_node(&state, kind, "b", "second", "", &["alpha", "beta"]).await;

        let listed = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                properties: None,
            }),
        )
        .await
        .expect("list nodes");
        assert_eq!(listed.items.len(), 2);
        assert!(listed.items.iter().all(|node| !node.tags.is_empty()));

        let tags = list_tags(State(state.clone())).await.expect("list tags");
        assert_eq!(tags.items[0].tag, "alpha");
        assert_eq!(tags.items[0].node_count, 2);
    }

    #[tokio::test]
    async fn replace_node_tags_swaps_the_set() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let node = new_node(&state, kind, "a", "first", "", &["alpha"]).await;

        let replaced = replace_node_tags(
            State(state.clone()),
            Path(node.id),
            Json(ReplaceTagsParameters {
                tags: vec!["Beta".to_string()],
            }),
        )
        .await
        .expect("replace tags");
        assert_eq!(replaced.items, vec!["beta"]);

        let fetched = get_node(State(state.clone()), Path(node.id))
            .await
            .expect("get node")
            .data;
        assert_eq!(fetched.tags, vec!["beta"]);
    }

    #[tokio::test]
    async fn search_covers_description_and_content() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let described =
            new_node(&state, kind, "described", "refresh token rotation", "", &[]).await;
        let bodied = new_node(
            &state,
            kind,
            "bodied",
            "unrelated",
            "refresh token rotation",
            &[],
        )
        .await;

        let found = search_handler(
            &state,
            SearchParameters {
                q: Some("refresh rotation".to_string()),
                tags: None,
                properties: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .expect("search");
        let ids: Vec<i64> = found.data.items.iter().map(|hit| hit.node.id).collect();
        assert!(ids.contains(&described.id));
        assert!(ids.contains(&bodied.id));
        assert_eq!(found.data.total, 2);
    }

    #[tokio::test]
    async fn search_requires_all_tags_and_accepts_tag_only_queries() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let both = new_node(&state, kind, "both", "shared text", "", &["alpha", "beta"]).await;
        new_node(&state, kind, "only-alpha", "shared text", "", &["alpha"]).await;

        let tag_only = search_handler(
            &state,
            SearchParameters {
                q: None,
                tags: Some("alpha,beta".to_string()),
                properties: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .expect("tag only search");
        assert_eq!(tag_only.data.items.len(), 1);
        assert_eq!(tag_only.data.items[0].node.id, both.id);

        let combined = search_handler(
            &state,
            SearchParameters {
                q: Some("shared".to_string()),
                tags: Some("alpha".to_string()),
                properties: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .expect("combined search");
        assert_eq!(combined.data.total, 2);
    }

    #[tokio::test]
    async fn search_defaults_and_clamps_paging() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        for index in 0..3 {
            new_node(&state, kind, &format!("n{index}"), "paged text", "", &[]).await;
        }

        let page = search_handler(
            &state,
            SearchParameters {
                q: Some("paged".to_string()),
                tags: None,
                properties: None,
                limit: Some(2),
                offset: Some(1),
            },
        )
        .await
        .expect("paged search");
        assert_eq!(page.data.items.len(), 2);
        assert_eq!(page.data.total, 3);
        assert_eq!(page.data.limit, 2);
        assert_eq!(page.data.offset, 1);

        let clamped = search_handler(
            &state,
            SearchParameters {
                q: Some("paged".to_string()),
                tags: None,
                properties: None,
                limit: Some(10_000),
                offset: None,
            },
        )
        .await
        .expect("clamped search");
        assert_eq!(clamped.data.limit, SearchConfig::default().max_limit);
    }

    #[tokio::test]
    async fn search_rejects_an_empty_query_without_tags() {
        let state = test_state().await;
        let error = search_handler(
            &state,
            SearchParameters {
                q: Some("   ".to_string()),
                tags: None,
                properties: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .expect_err("empty search rejected");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn update_task_requires_a_task_node() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let node = new_node(&state, kind, "a", "first", "", &[]).await;
        let error = update_task(
            State(state.clone()),
            Path(node.id),
            Json(UpdateTaskParameters {
                name: Some("b".to_string()),
                description: None,
                content: None,
            }),
        )
        .await
        .expect_err("non-task rejected");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn unknown_node_tags_are_not_found() {
        let state = test_state().await;
        let error = get_node_tags(State(state.clone()), Path(999))
            .await
            .expect_err("missing node rejected");
        assert_eq!(error.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn applies_a_pack_and_reports_statuses() {
        let state = test_state().await;
        let pack = |description: &str| ApplyPackParameters {
            node_kinds: vec![PackNodeKindParameters {
                name: "widget".to_string(),
                description: Some(description.to_string()),
            }],
            edge_types: vec![PackEdgeTypeParameters {
                name: "widget_part".to_string(),
                description: Some("widget -> widget".to_string()),
                resolves: None,
                single_outgoing: None,
                acyclic: None,
                source_kinds: Some(vec!["widget".to_string()]),
                destination_kinds: Some(vec!["widget".to_string()]),
            }],
        };

        let first = apply_pack(State(state.clone()), Json(pack("a widget")))
            .await
            .expect("first apply")
            .data;
        assert_eq!((first.created, first.updated, first.unchanged), (2, 0, 0));
        assert_eq!(first.node_kinds[0].status, "created");

        let second = apply_pack(State(state.clone()), Json(pack("a widget")))
            .await
            .expect("second apply")
            .data;
        assert_eq!(
            (second.created, second.updated, second.unchanged),
            (0, 0, 2)
        );
        assert_eq!(second.node_kinds[0].version, Some(1));
        assert_eq!(second.edge_types[0].status, "unchanged");

        let third = apply_pack(State(state.clone()), Json(pack("a changed widget")))
            .await
            .expect("third apply")
            .data;
        assert_eq!((third.updated, third.unchanged), (1, 1));
        assert_eq!(third.node_kinds[0].version, Some(2));
        assert_eq!(third.node_kinds[0].status, "updated");
    }

    #[tokio::test]
    async fn imports_nodes_and_edges_over_http() {
        let state = test_state().await;
        seed_kind(&state, "decision").await;
        let params = || ImportParameters {
            on_conflict: ImportConflictParameters::Update,
            nodes: vec![ImportNodeParameters {
                reference: Some("n1".to_string()),
                kind: "decision".to_string(),
                name: "use sqlite".to_string(),
                description: Some("single file".to_string()),
                content: Some("portable".to_string()),
                tags: Some(vec!["Repo:X".to_string()]),
                properties: None,
            }],
            edges: vec![],
        };

        let first = import_graph(State(state.clone()), Json(params()))
            .await
            .expect("import")
            .data;
        assert_eq!(first.nodes_created, 1);
        assert_eq!(first.nodes[0].reference.as_deref(), Some("n1"));
        assert_eq!(first.nodes[0].status, "created");

        let second = import_graph(State(state.clone()), Json(params()))
            .await
            .expect("re-import")
            .data;
        assert_eq!(second.nodes_created, 0);
        assert_eq!(second.nodes_unchanged, 1);
        assert_eq!(second.nodes[0].id, first.nodes[0].id);
    }

    #[test]
    fn update_node_rejects_unknown_fields() {
        assert!(serde_json::from_str::<UpdateNodeParameters>(r#"{"kind_id":1}"#).is_err());
        assert!(serde_json::from_str::<UpdateNodeParameters>(r#"{"description":"x"}"#).is_ok());
    }

    #[tokio::test]
    async fn updates_a_node_over_http() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let node = new_node(&state, kind, "first", "original", "body", &["one"]).await;

        let updated = update_node(
            State(state.clone()),
            Path(node.id),
            Json(UpdateNodeParameters {
                name: Some("second".to_string()),
                description: Some("changed".to_string()),
                content: None,
                tags: Some(vec!["Two".to_string()]),
                properties: None,
            }),
        )
        .await
        .expect("update")
        .data;
        assert_eq!(updated.name, "second");
        assert_eq!(updated.description, "changed");
        assert_eq!(updated.content, "body");
        assert_eq!(updated.tags, vec!["two"]);

        let fetched = get_node(State(state.clone()), Path(node.id))
            .await
            .expect("get node")
            .data;
        assert_eq!(fetched.name, "second");
    }

    #[tokio::test]
    async fn node_properties_round_trip_over_http() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let created = create_node(
            State(state.clone()),
            Json(CreateNodeParameters {
                kind_id: kind,
                name: "use sqlite".to_string(),
                description: "portable database".to_string(),
                content: String::new(),
                tags: vec![],
                properties: Some(serde_json::json!({
                    "confidence": 0.9,
                    "source": "docs"
                })),
            }),
        )
        .await
        .expect("create")
        .data;
        assert_eq!(
            created.properties,
            serde_json::json!({"confidence": 0.9, "source": "docs"})
        );

        let updated = update_node(
            State(state.clone()),
            Path(created.id),
            Json(UpdateNodeParameters {
                name: None,
                description: None,
                content: None,
                tags: None,
                properties: Some(serde_json::json!({"status": "verified"})),
            }),
        )
        .await
        .expect("update")
        .data;
        assert_eq!(
            updated.properties,
            serde_json::json!({"status": "verified"})
        );

        let fetched = get_node(State(state.clone()), Path(created.id))
            .await
            .expect("get")
            .data;
        assert_eq!(
            fetched.properties,
            serde_json::json!({"status": "verified"})
        );

        let listed = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                properties: None,
            }),
        )
        .await
        .expect("list");
        assert_eq!(
            listed.items[0].properties,
            serde_json::json!({"status": "verified"})
        );

        let found = search(
            State(state.clone()),
            Query(SearchParameters {
                q: Some("portable".to_string()),
                tags: None,
                properties: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("search");
        assert_eq!(
            found.data.items[0].node.properties,
            serde_json::json!({"status": "verified"})
        );
    }

    #[tokio::test]
    async fn filters_list_and_search_by_properties_over_http() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        create_node(
            State(state.clone()),
            Json(CreateNodeParameters {
                kind_id: kind,
                name: "verified".to_string(),
                description: "portable database".to_string(),
                content: String::new(),
                tags: vec![],
                properties: Some(serde_json::json!({"status": "verified"})),
            }),
        )
        .await
        .expect("create verified");
        create_node(
            State(state.clone()),
            Json(CreateNodeParameters {
                kind_id: kind,
                name: "draft".to_string(),
                description: "portable notes".to_string(),
                content: String::new(),
                tags: vec![],
                properties: Some(serde_json::json!({"status": "draft"})),
            }),
        )
        .await
        .expect("create draft");

        let listed = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                properties: Some(r#"{"status":"verified"}"#.to_string()),
            }),
        )
        .await
        .expect("filtered list");
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].name, "verified");

        let found = search(
            State(state.clone()),
            Query(SearchParameters {
                q: Some("portable".to_string()),
                tags: None,
                properties: Some(r#"{"status":"draft"}"#.to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("filtered search");
        assert_eq!(found.data.items.len(), 1);
        assert_eq!(found.data.items[0].node.name, "draft");

        let error = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                properties: Some("[1]".to_string()),
            }),
        )
        .await
        .expect_err("non-object filter");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn edge_properties_round_trip_over_http() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        let first = new_node(&state, kind, "first", "", "", &[]).await;
        let second = new_node(&state, kind, "second", "", "", &[]).await;
        let edge_type = state
            .graph_service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "relates".to_string(),
                description: "decision -> decision".to_string(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: Some("decision".to_string()),
                destination_kinds: Some("decision".to_string()),
                updated_at: String::new(),
            })
            .await
            .expect("edge type");

        add_edge(
            State(state.clone()),
            Json(CreateEdgeParameters {
                source: first.id,
                destination: second.id,
                edge_type_id: edge_type.id,
                properties: Some(serde_json::json!({"weight": 2})),
            }),
        )
        .await
        .expect("add edge");

        let neighbors = list_neighbors(
            State(state.clone()),
            Path(first.id),
            Query(NeighborParameters {
                direction: NeighborDirection::Outgoing,
                edge_type: Some("relates".to_string()),
            }),
        )
        .await
        .expect("neighbors");
        assert_eq!(neighbors.items.len(), 1);
        assert_eq!(
            neighbors.items[0].edge.properties,
            serde_json::json!({"weight": 2})
        );
    }
}
