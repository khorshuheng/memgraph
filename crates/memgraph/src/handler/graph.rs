use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
};

use crate::{
    dto::{
        graph::{
            ApplyPackParameters, CreateChangeParameters, CreateEdgeParameters,
            CreateEdgeTypeParameters, CreateNodeKindParameters, CreateNodeParameters,
            CreatePlanParameters, CreateTaskParameters, DeleteEdgeParameters, EdgeListParameters,
            EdgeListResponse, EdgeTypeResponse, FillNames, GoalTreeResponse, GraphExportResponse,
            ImportParameters, ImportReportResponse, ListNodesParameters, ListNodesResponse,
            NeighborParameters, NeighborResponse, NodeKindResponse, NodeResponse,
            PackReportResponse, PageParameters, PlanResponse, PlanTaskResponse, QueryParameters,
            QueryResponse, ReplaceTagsParameters, SearchHitResponse, SearchParameters,
            SearchResponse, TagUsageResponse, UpdateEdgeParameters, UpdateNodeParameters,
            UpdateTaskParameters, VocabularyResponse, WorkListResponse, WorkParameters,
            WorkTaskResponse,
        },
        health::GraphHealthResponse,
        wrapper::{ApiError, ApiList, ApiSuccess},
    },
    model::{
        ChangeDraft, Edge, EdgeFilter, EdgeType, ImportDraft, Node, NodeFilter, NodeKind,
        NodePatch, PackDraft, PlanDraft, QueryRequest, TaskDraft, TaskPatch, WorkFilter,
    },
    state::AppState,
};

const NODE_PROJECTION_FIELDS: [&str; 11] = [
    "id",
    "kind_id",
    "kind",
    "name",
    "description",
    "content",
    "tags",
    "properties",
    "version",
    "created_at",
    "updated_at",
];
const WORK_PROJECTION_FIELDS: [&str; 7] = [
    "id",
    "name",
    "status",
    "updated_at",
    "goal",
    "depends_on",
    "implemented_by",
];

fn parse_fields(raw: Option<&str>) -> Result<Vec<String>, ApiError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut fields = Vec::new();
    for field in raw.split(',') {
        let field = field.trim();
        if field.is_empty() {
            return Err(
                ApiError::unprocessable("fields must not contain blank entries")
                    .with_field("fields"),
            );
        }
        if !fields.iter().any(|existing| existing == field) {
            fields.push(field.to_string());
        }
    }
    Ok(fields)
}

fn project_value(
    value: serde_json::Value,
    fields: &[String],
    allowed: &[&str],
    label: &str,
) -> Result<serde_json::Value, ApiError> {
    if fields.is_empty() {
        return Ok(value);
    }
    let object = value.as_object().cloned().unwrap_or_default();
    let mut projected = serde_json::Map::new();
    for field in fields {
        if !allowed.contains(&field.as_str()) {
            return Err(ApiError::unprocessable(format!(
                "field '{field}' is not projectable; allowed: {}",
                allowed.join(", ")
            ))
            .with_field(label));
        }
        if let Some(value) = object.get(field) {
            projected.insert(field.clone(), value.clone());
        }
    }
    Ok(serde_json::Value::Object(projected))
}

fn project_node(node: NodeResponse, fields: &[String]) -> Result<serde_json::Value, ApiError> {
    project_value(
        serde_json::to_value(node).unwrap_or(serde_json::Value::Null),
        fields,
        &NODE_PROJECTION_FIELDS,
        "fields",
    )
}

fn project_work(task: WorkTaskResponse, fields: &[String]) -> Result<serde_json::Value, ApiError> {
    project_value(
        serde_json::to_value(task).unwrap_or(serde_json::Value::Null),
        fields,
        &WORK_PROJECTION_FIELDS,
        "fields",
    )
}

async fn fill_names<T: FillNames>(state: &AppState, data: &mut T) -> Result<(), ApiError> {
    let (kinds, edge_types) = state.graph_service.name_maps().await?;
    data.fill_names(&kinds, &edge_types);
    Ok(())
}

fn parse_if_match(headers: &HeaderMap) -> Result<Option<i64>, ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let raw = value.to_str().map_err(|_| {
        ApiError::unprocessable("If-Match must be a numeric version or *").with_field("if-match")
    })?;
    let raw = raw.trim();
    if raw == "*" {
        return Ok(None);
    }
    if raw.contains(',') {
        return Err(
            ApiError::unprocessable("If-Match must contain a single version or *")
                .with_field("if-match"),
        );
    }
    let raw = raw
        .strip_prefix("W/")
        .unwrap_or(raw)
        .trim()
        .trim_matches('"');
    let version = raw.parse::<i64>().map_err(|_| {
        ApiError::unprocessable("If-Match must be a numeric version or *").with_field("if-match")
    })?;
    Ok(Some(version))
}

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
    let ids: Vec<i64> = node_kinds.iter().map(|kind| kind.id).collect();
    let mut properties = state
        .graph_service
        .node_kind_properties_for_ids(&ids)
        .await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items: node_kinds
            .into_iter()
            .map(|kind| {
                let declared = properties
                    .remove(&kind.id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(Into::into)
                    .collect();
                NodeKindResponse::with_properties(kind, declared)
            })
            .collect(),
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
    get,
    path = "/vocabulary",
    responses((status = 200, description = "Node kinds, edge types and declared properties", body = VocabularyResponse))
)]
pub async fn vocabulary(
    State(state): State<AppState>,
) -> Result<ApiSuccess<VocabularyResponse>, ApiError> {
    let node_kinds = state.graph_service.list_node_kinds().await?;
    let ids: Vec<i64> = node_kinds.iter().map(|kind| kind.id).collect();
    let mut properties = state
        .graph_service
        .node_kind_properties_for_ids(&ids)
        .await?;
    let kinds = node_kinds
        .into_iter()
        .map(|kind| {
            let declared = properties
                .remove(&kind.id)
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect();
            NodeKindResponse::with_properties(kind, declared)
        })
        .collect();
    let edge_types = state
        .graph_service
        .list_edge_types()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: VocabularyResponse { kinds, edge_types },
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
    let mut data = NodeResponse::with_tags_and_properties(created, tags, properties);
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data,
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
    let tags = state.graph_service.node_tags(created.id).await?;
    let properties = state.graph_service.node_properties(created.id).await?;
    let mut data = NodeResponse::with_tags_and_properties(created, tags, properties);
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data,
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/changes",
    request_body = CreateChangeParameters,
    responses(
        (status = 201, description = "Change recorded", body = NodeResponse),
        (status = 404, description = "A referenced task was not found"),
        (status = 409, description = "A change with this commit already exists"),
        (status = 422, description = "A referenced file is unknown, a node has the wrong kind, or properties violate the change kind")
    )
)]
pub async fn create_change(
    State(state): State<AppState>,
    Json(params): Json<CreateChangeParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let draft: ChangeDraft = params.into();
    let created = state.graph_service.create_change(&draft).await?;
    let tags = state.graph_service.node_tags(created.id).await?;
    let properties = state.graph_service.node_properties(created.id).await?;
    let mut data = NodeResponse::with_tags_and_properties(created, tags, properties);
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data,
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
    let ids: Vec<i64> = plan
        .goal
        .iter()
        .map(|goal| goal.id)
        .chain(plan.tasks.iter().map(|task| task.node.id))
        .collect();
    let mut tags = state.graph_service.node_tags_for_ids(&ids).await?;
    let mut properties = state.graph_service.node_properties_for_ids(&ids).await?;
    let goal = plan.goal.map(|goal| {
        let tags = tags.remove(&goal.id).unwrap_or_default();
        let properties = properties
            .remove(&goal.id)
            .unwrap_or_else(|| serde_json::json!({}));
        NodeResponse::with_tags_and_properties(goal, tags, properties)
    });
    let tasks = plan
        .tasks
        .into_iter()
        .map(|task| {
            let tags = tags.remove(&task.node.id).unwrap_or_default();
            let properties = properties
                .remove(&task.node.id)
                .unwrap_or_else(|| serde_json::json!({}));
            PlanTaskResponse {
                key: task.key,
                node: NodeResponse::with_tags_and_properties(task.node, tags, properties),
            }
        })
        .collect();
    let mut data = PlanResponse { goal, tasks };
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::CREATED,
        data,
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
    responses((status = 200, description = "List nodes", body = ListNodesResponse))
)]
pub async fn list_nodes(
    State(state): State<AppState>,
    Query(params): Query<ListNodesParameters>,
) -> Result<ApiSuccess<ListNodesResponse>, ApiError> {
    let ids = params
        .id_list()
        .map_err(|message| ApiError::unprocessable(message).with_field("ids"))?;
    let fields = parse_fields(params.fields.as_deref())?;
    let order = params
        .order_list()
        .map_err(|message| ApiError::unprocessable(message).with_field("order"))?;
    let since = params.since.clone();
    if let Some(ids) = ids {
        if params.kind.is_some()
            || params.name.is_some()
            || params.name_contains.is_some()
            || params.scope.is_some()
            || params.within.is_some()
            || params.via.is_some()
            || params.descend.is_some()
            || params.properties.is_some()
            || !order.is_empty()
            || since.is_some()
            || params.limit.is_some()
            || params.offset.is_some()
        {
            return Err(
                ApiError::unprocessable("ids cannot be combined with other node filters")
                    .with_field("ids"),
            );
        }
        let nodes = state.graph_service.nodes_by_ids(&ids).await?;
        let total = nodes.len() as i64;
        let mut items: Vec<NodeResponse> = nodes
            .into_iter()
            .map(|item| {
                NodeResponse::with_tags_and_properties(item.node, item.tags, item.properties)
            })
            .collect();
        fill_names(&state, &mut items).await?;
        let items = items
            .into_iter()
            .map(|node| project_node(node, &fields))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(ApiSuccess {
            status: StatusCode::OK,
            data: ListNodesResponse {
                items,
                total,
                limit: total,
                offset: 0,
            },
        });
    }
    let limit = params.limit;
    let offset = params.offset;
    let properties = params
        .property_filter()
        .map_err(|message| ApiError::unprocessable(message).with_field("properties"))?;
    let mut filter: NodeFilter = params.into();
    filter.properties = properties;
    let page = state
        .graph_service
        .list_nodes_page_ordered(&filter, limit, offset, &order, since.as_deref())
        .await?;
    let ids: Vec<i64> = page.items.iter().map(|node| node.id).collect();
    let mut tags = state.graph_service.node_tags_for_ids(&ids).await?;
    let mut properties = state.graph_service.node_properties_for_ids(&ids).await?;
    let mut items: Vec<NodeResponse> = page
        .items
        .into_iter()
        .map(|node| {
            let node_tags = tags.remove(&node.id).unwrap_or_default();
            let node_properties = properties
                .remove(&node.id)
                .unwrap_or_else(|| serde_json::json!({}));
            NodeResponse::with_tags_and_properties(node, node_tags, node_properties)
        })
        .collect();
    fill_names(&state, &mut items).await?;
    let items = items
        .into_iter()
        .map(|node| project_node(node, &fields))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: ListNodesResponse {
            items,
            total: page.total,
            limit: page.limit,
            offset: page.offset,
        },
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
    headers: HeaderMap,
    Json(params): Json<UpdateTaskParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let patch: TaskPatch = params.into();
    let expected = parse_if_match(&headers)?;
    let updated = match expected {
        Some(version) => {
            state
                .graph_service
                .update_task_checked(task_id, &patch, version)
                .await?
        }
        None => state.graph_service.update_task(task_id, &patch).await?,
    };
    let tags = state.graph_service.node_tags(updated.id).await?;
    let properties = state.graph_service.node_properties(updated.id).await?;
    let mut data = NodeResponse::with_tags_and_properties(updated, tags, properties);
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
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
    let mut data = NodeResponse::with_tags_and_properties(node, tags, properties);
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
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
    headers: HeaderMap,
    Json(params): Json<UpdateNodeParameters>,
) -> Result<ApiSuccess<NodeResponse>, ApiError> {
    let patch: NodePatch = params.into();
    let expected = parse_if_match(&headers)?;
    let (node, tags) = match expected {
        Some(version) => {
            state
                .graph_service
                .update_node_checked(node_id, &patch, version)
                .await?
        }
        None => state.graph_service.update_node(node_id, &patch).await?,
    };
    let properties = state.graph_service.node_properties(node_id).await?;
    let mut data = NodeResponse::with_tags_and_properties(node, tags, properties);
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
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
    path = "/work/pending",
    params(WorkParameters),
    responses(
        (status = 200, description = "Unfinished tasks with their dependency state", body = WorkListResponse),
        (status = 404, description = "Goal or repo filter not found"),
        (status = 422, description = "Filter node has the wrong kind")
    )
)]
pub async fn work_pending(
    State(state): State<AppState>,
    Query(params): Query<WorkParameters>,
) -> Result<ApiSuccess<WorkListResponse>, ApiError> {
    let fields = parse_fields(params.fields.as_deref())?;
    let limit = params.limit;
    let offset = params.offset;
    let filter: WorkFilter = params.into();
    let page = state
        .graph_service
        .work_report(&filter, false, limit, offset)
        .await?;
    let items = page
        .items
        .into_iter()
        .map(WorkTaskResponse::from)
        .map(|task| project_work(task, &fields))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: WorkListResponse {
            items,
            total: page.total,
            limit: page.limit,
            offset: page.offset,
        },
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/work/next",
    params(WorkParameters),
    responses(
        (status = 200, description = "Unfinished tasks whose dependencies are all done", body = WorkListResponse),
        (status = 404, description = "Goal or repo filter not found"),
        (status = 422, description = "Filter node has the wrong kind")
    )
)]
pub async fn work_next(
    State(state): State<AppState>,
    Query(params): Query<WorkParameters>,
) -> Result<ApiSuccess<WorkListResponse>, ApiError> {
    let fields = parse_fields(params.fields.as_deref())?;
    let limit = params.limit;
    let offset = params.offset;
    let filter: WorkFilter = params.into();
    let page = state
        .graph_service
        .work_report(&filter, true, limit, offset)
        .await?;
    let items = page
        .items
        .into_iter()
        .map(WorkTaskResponse::from)
        .map(|task| project_work(task, &fields))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: WorkListResponse {
            items,
            total: page.total,
            limit: page.limit,
            offset: page.offset,
        },
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/goals/{goal_id}/tree",
    params(PageParameters, ("goal_id" = i64, Path, description = "Goal id")),
    responses(
        (status = 200, description = "Goal with its task tree and status", body = GoalTreeResponse),
        (status = 404, description = "Goal not found"),
        (status = 422, description = "Node is not a goal")
    )
)]
pub async fn goal_tree(
    State(state): State<AppState>,
    Path(goal_id): Path<i64>,
    Query(params): Query<PageParameters>,
) -> Result<ApiSuccess<GoalTreeResponse>, ApiError> {
    let fields = parse_fields(params.fields.as_deref())?;
    let (goal, page) = state
        .graph_service
        .goal_tree(goal_id, params.limit, params.offset)
        .await?;
    let tags = state.graph_service.node_tags(goal.id).await?;
    let properties = state.graph_service.node_properties(goal.id).await?;
    let tasks = page
        .items
        .into_iter()
        .map(WorkTaskResponse::from)
        .map(|task| project_work(task, &fields))
        .collect::<Result<Vec<_>, _>>()?;
    let mut data = GoalTreeResponse {
        goal: NodeResponse::with_tags_and_properties(goal, tags, properties),
        tasks,
        total: page.total,
        limit: page.limit,
        offset: page.offset,
    };
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
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
    let mut items: Vec<NeighborResponse> = neighbors
        .into_iter()
        .map(|neighbor| {
            let node_tags = tags.remove(&neighbor.node.id).unwrap_or_default();
            let node_properties = properties
                .remove(&neighbor.node.id)
                .unwrap_or_else(|| serde_json::json!({}));
            NeighborResponse::with_tags_and_properties(neighbor, node_tags, node_properties)
        })
        .collect();
    fill_names(&state, &mut items).await?;
    Ok(ApiList {
        status: StatusCode::OK,
        items,
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/edges",
    params(EdgeListParameters),
    responses(
        (status = 200, description = "Edges matching the filters", body = EdgeListResponse),
        (status = 422, description = "Edge type filter is not defined")
    )
)]
pub async fn list_edges(
    State(state): State<AppState>,
    Query(params): Query<EdgeListParameters>,
) -> Result<ApiSuccess<EdgeListResponse>, ApiError> {
    let limit = params.limit;
    let offset = params.offset;
    let filter: EdgeFilter = params.into();
    let page = state
        .graph_service
        .list_edges(&filter, limit, offset)
        .await?;
    let mut data = EdgeListResponse {
        items: page.items.into_iter().map(Into::into).collect(),
        total: page.total,
        limit: page.limit,
        offset: page.offset,
    };
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
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
    let edge = Edge {
        source: state
            .graph_service
            .resolve_node_selector(&params.source.into())
            .await?,
        destination: state
            .graph_service
            .resolve_node_selector(&params.destination.into())
            .await?,
        edge_type_id: state
            .graph_service
            .resolve_edge_type(params.edge_type_id, params.edge_type.as_deref())
            .await?,
        created_at: String::new(),
    };
    state
        .graph_service
        .add_edge_with_properties(&edge, properties.as_ref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Graph",
    patch,
    path = "/edges",
    request_body = UpdateEdgeParameters,
    responses(
        (status = 204, description = "Edge properties updated"),
        (status = 404, description = "Edge not found"),
        (status = 422, description = "Properties must be a JSON object")
    )
)]
pub async fn update_edge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(params): Json<UpdateEdgeParameters>,
) -> Result<StatusCode, ApiError> {
    let edge = Edge {
        source: params.source,
        destination: params.destination,
        edge_type_id: params.edge_type_id,
        created_at: String::new(),
    };
    let expected = parse_if_match(&headers)?;
    match expected {
        Some(version) => {
            state
                .graph_service
                .set_edge_properties_checked(&edge, &params.properties, version)
                .await?
        }
        None => {
            state
                .graph_service
                .set_edge_properties(&edge, &params.properties)
                .await?
        }
    }
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
    let properties = params
        .property_filter()
        .map_err(|message| ApiError::unprocessable(message).with_field("properties"))?;
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
    let mut data = SearchResponse {
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
    };
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
    })
}

#[utoipa::path(
    tag = "Graph",
    post,
    path = "/query",
    request_body = QueryParameters,
    responses(
        (status = 200, description = "Read-only node or edge query", body = QueryResponse),
        (status = 422, description = "Unknown filter, field, order or vocabulary name")
    )
)]
pub async fn query(
    State(state): State<AppState>,
    Json(params): Json<QueryParameters>,
) -> Result<ApiSuccess<QueryResponse>, ApiError> {
    let request: QueryRequest = params.into();
    let page = state.graph_service.query(&request).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: page.into(),
    })
}

#[utoipa::path(
    tag = "Graph",
    get,
    path = "/graph/export",
    responses(
        (status = 200, description = "Compact node and edge snapshot", body = GraphExportResponse),
        (status = 413, description = "Graph exceeds the export limits")
    )
)]
pub async fn graph_export(
    State(state): State<AppState>,
) -> Result<ApiSuccess<GraphExportResponse>, ApiError> {
    let export = state.graph_service.export_graph().await?;
    let mut data: GraphExportResponse = export.into();
    fill_names(&state, &mut data).await?;
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data,
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
    use crate::config::{AppConfig, DatabaseConfig, PagingConfig, ServerConfig};
    use crate::dto::graph::{
        CreatePlanGoalParameters, CreatePlanTaskParameters, ImportConflictParameters,
        ImportNodeParameters, NeighborDirection, NodeSelectorParameters, PackEdgeTypeParameters,
        PackNodeKindParameters, PackPropertyParameters, PlanAnchorParameters,
    };
    use crate::model::{PackNodeKind, PackProperty};

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
            paging: PagingConfig::default(),
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
                unique_name: true,
                strict_properties: false,
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

    async fn seed_anchored_goal(state: &AppState) -> i64 {
        let repo_kind = seed_kind(state, "repo").await;
        seed_kind(state, "goal").await;
        seed_kind(state, "task").await;
        seed_kind(state, "file").await;
        seed_kind(state, "change").await;
        for (name, source, destination) in [
            ("belongs_to", "goal", "repo"),
            ("part_of", "task", "task,goal"),
            ("depends_on", "task", "task"),
            ("affects", "task", "file"),
            ("implements", "change", "task"),
            ("modifies", "change", "file"),
        ] {
            state
                .graph_service
                .create_edge_type(&EdgeType {
                    id: 0,
                    name: name.to_string(),
                    description: String::new(),
                    resolves: false,
                    single_outgoing: false,
                    acyclic: false,
                    source_kinds: Some(source.to_string()),
                    destination_kinds: Some(destination.to_string()),
                    updated_at: String::new(),
                })
                .await
                .expect("edge type");
        }
        let repo = new_node(state, repo_kind, "acme/widget", "", "", &[]).await;
        create_plan(
            State(state.clone()),
            Json(CreatePlanParameters {
                goal: Some(CreatePlanGoalParameters {
                    id: None,
                    name: "ship".to_string(),
                    description: String::new(),
                    content: String::new(),
                    properties: None,
                    tags: None,
                }),
                goal_id: None,
                anchor: Some(PlanAnchorParameters {
                    node_id: repo.id,
                    edge_type: "belongs_to".to_string(),
                }),
                tasks: vec![],
            }),
        )
        .await
        .expect("create anchored goal")
        .data
        .goal
        .expect("goal")
        .id
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
        assert!(!created.created_at.is_empty());

        let fetched = get_node(State(state.clone()), Path(created.id))
            .await
            .expect("get node")
            .data;
        assert_eq!(fetched.tags, vec!["repo:x", "topic:storage"]);
    }

    #[tokio::test]
    async fn create_and_update_task_expose_and_replace_tags() {
        let state = test_state().await;
        let goal = seed_anchored_goal(&state).await;

        let created = create_task(
            State(state.clone()),
            Json(CreateTaskParameters {
                id: None,
                name: "tagged task".to_string(),
                description: String::new(),
                content: String::new(),
                parent: Some(goal),
                depends_on: vec![],
                affects: vec![],
                properties: None,
                tags: Some(vec!["Repo:X".to_string(), " Phase:Plan ".to_string()]),
            }),
        )
        .await
        .expect("create task")
        .data;
        assert_eq!(created.tags, vec!["phase:plan", "repo:x"]);

        let updated = update_task(
            State(state.clone()),
            Path(created.id),
            HeaderMap::new(),
            Json(UpdateTaskParameters {
                name: None,
                description: Some("changed".to_string()),
                content: None,
                properties: None,
                parent: None,
                depends_on: None,
                affects: None,
                tags: Some(vec!["Other".to_string()]),
            }),
        )
        .await
        .expect("update task")
        .data;
        assert_eq!(updated.tags, vec!["other"]);

        let preserved = update_task(
            State(state.clone()),
            Path(created.id),
            HeaderMap::new(),
            Json(UpdateTaskParameters {
                name: None,
                description: Some("again".to_string()),
                content: None,
                properties: None,
                parent: None,
                depends_on: None,
                affects: None,
                tags: None,
            }),
        )
        .await
        .expect("update task")
        .data;
        assert_eq!(preserved.tags, vec!["other"]);

        let cleared = update_task(
            State(state.clone()),
            Path(created.id),
            HeaderMap::new(),
            Json(UpdateTaskParameters {
                name: None,
                description: None,
                content: None,
                properties: None,
                parent: None,
                depends_on: None,
                affects: None,
                tags: Some(vec![]),
            }),
        )
        .await
        .expect("clear tags")
        .data;
        assert!(cleared.tags.is_empty());
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
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("list nodes");
        assert_eq!(listed.data.items.len(), 2);
        assert!(
            listed
                .data
                .items
                .iter()
                .all(|node| node["tags"].as_array().is_some_and(|tags| !tags.is_empty()))
        );
        assert_eq!(listed.data.total, 2);

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
        assert_eq!(clamped.data.limit, PagingConfig::default().max_limit);
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
            HeaderMap::new(),
            Json(UpdateTaskParameters {
                name: Some("b".to_string()),
                description: None,
                content: None,
                properties: None,
                parent: None,
                depends_on: None,
                affects: None,
                tags: None,
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
                unique_name: None,
                properties: None,
                strict_properties: None,
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
                id: None,
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
            HeaderMap::new(),
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
            HeaderMap::new(),
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
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("list");
        assert_eq!(
            listed.data.items[0]["properties"],
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
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: Some(r#"{"status":"verified"}"#.to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("filtered list");
        assert_eq!(listed.data.items.len(), 1);
        assert_eq!(listed.data.items[0]["name"], "verified");

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
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: Some("[1]".to_string()),
                limit: None,
                offset: None,
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
                source: NodeSelectorParameters::Id(first.id),
                destination: NodeSelectorParameters::Id(second.id),
                edge_type_id: Some(edge_type.id),
                edge_type: None,
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

    #[tokio::test]
    async fn updates_edge_properties_over_http() {
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
                source: NodeSelectorParameters::Id(first.id),
                destination: NodeSelectorParameters::Id(second.id),
                edge_type_id: Some(edge_type.id),
                edge_type: None,
                properties: None,
            }),
        )
        .await
        .expect("add edge");

        let status = update_edge(
            State(state.clone()),
            HeaderMap::new(),
            Json(UpdateEdgeParameters {
                source: first.id,
                destination: second.id,
                edge_type_id: edge_type.id,
                properties: serde_json::json!({"weight": 3}),
            }),
        )
        .await
        .expect("update edge");
        assert_eq!(status, StatusCode::NO_CONTENT);

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
        assert_eq!(
            neighbors.items[0].edge.properties,
            serde_json::json!({"weight": 3})
        );

        update_edge(
            State(state.clone()),
            HeaderMap::new(),
            Json(UpdateEdgeParameters {
                source: first.id,
                destination: second.id,
                edge_type_id: edge_type.id,
                properties: serde_json::json!({}),
            }),
        )
        .await
        .expect("clear edge");

        let missing = update_edge(
            State(state.clone()),
            HeaderMap::new(),
            Json(UpdateEdgeParameters {
                source: second.id,
                destination: first.id,
                edge_type_id: edge_type.id,
                properties: serde_json::json!({}),
            }),
        )
        .await
        .expect_err("missing edge");
        assert_eq!(missing.status, StatusCode::NOT_FOUND);

        let invalid = update_edge(
            State(state.clone()),
            HeaderMap::new(),
            Json(UpdateEdgeParameters {
                source: first.id,
                destination: second.id,
                edge_type_id: edge_type.id,
                properties: serde_json::json!([1]),
            }),
        )
        .await
        .expect_err("invalid properties");
        assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn node_kind_responses_include_declared_properties() {
        let state = test_state().await;
        state
            .graph_service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "widget".to_string(),
                    description: Some("a widget".to_string()),
                    unique_name: None,
                    properties: Some(vec![PackProperty {
                        key: "status".to_string(),
                        value_type: Some("string".to_string()),
                        required: true,
                        indexed: true,
                    }]),
                    strict_properties: Some(true),
                }],
                edge_types: vec![],
            })
            .await
            .expect("apply pack");

        let kinds = list_node_kinds(State(state.clone()))
            .await
            .expect("list node kinds");
        let widget = kinds
            .items
            .iter()
            .find(|kind| kind.name == "widget")
            .expect("widget kind");
        assert_eq!(widget.properties.len(), 1);
        assert_eq!(widget.properties[0].key, "status");
        assert_eq!(widget.properties[0].value_type, "string");
        assert!(widget.properties[0].required);
        assert!(widget.properties[0].indexed);
        assert!(widget.strict_properties);
    }

    #[test]
    fn pack_property_type_rejects_unknown_values() {
        assert!(
            serde_json::from_str::<PackPropertyParameters>(r#"{"key":"x","value_type":"strange"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<PackPropertyParameters>(r#"{"key":"x","value_type":"string"}"#)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn plan_responses_include_task_and_goal_properties() {
        let state = test_state().await;
        let repo_kind = seed_kind(&state, "repo").await;
        seed_kind(&state, "goal").await;
        seed_kind(&state, "task").await;
        seed_kind(&state, "file").await;
        for (name, source, destination) in [
            ("belongs_to", "goal", "repo"),
            ("part_of", "task", "task,goal"),
            ("depends_on", "task", "task"),
            ("affects", "task", "file"),
        ] {
            state
                .graph_service
                .create_edge_type(&EdgeType {
                    id: 0,
                    name: name.to_string(),
                    description: String::new(),
                    resolves: false,
                    single_outgoing: false,
                    acyclic: false,
                    source_kinds: Some(source.to_string()),
                    destination_kinds: Some(destination.to_string()),
                    updated_at: String::new(),
                })
                .await
                .expect("edge type");
        }
        let repo = new_node(&state, repo_kind, "acme/widget", "", "", &[]).await;

        let plan = create_plan(
            State(state.clone()),
            Json(CreatePlanParameters {
                goal: Some(CreatePlanGoalParameters {
                    id: None,
                    name: "ship".to_string(),
                    description: String::new(),
                    content: String::new(),
                    properties: Some(serde_json::json!({"owner": "goal"})),
                    tags: Some(vec!["Goal:Ship".to_string()]),
                }),
                goal_id: None,
                anchor: Some(PlanAnchorParameters {
                    node_id: repo.id,
                    edge_type: "belongs_to".to_string(),
                }),
                tasks: vec![CreatePlanTaskParameters {
                    id: None,
                    key: "t".to_string(),
                    name: "task".to_string(),
                    description: String::new(),
                    content: String::new(),
                    parent: None,
                    depends_on: vec![],
                    affects: vec![],
                    properties: Some(serde_json::json!({"owner": "task"})),
                    tags: Some(vec!["Phase:Build".to_string()]),
                }],
            }),
        )
        .await
        .expect("create plan")
        .data;
        let goal = plan.goal.expect("goal");
        assert_eq!(goal.properties, serde_json::json!({"owner": "goal"}));
        assert_eq!(goal.tags, vec!["goal:ship"]);
        assert_eq!(
            plan.tasks[0].node.properties,
            serde_json::json!({"owner": "task"})
        );
        assert_eq!(plan.tasks[0].node.tags, vec!["phase:build"]);
    }

    #[tokio::test]
    async fn node_kind_responses_include_unique_name() {
        let state = test_state().await;
        state
            .graph_service
            .apply_pack(&PackDraft {
                node_kinds: vec![PackNodeKind {
                    name: "widget".to_string(),
                    description: Some("a widget".to_string()),
                    unique_name: Some(false),
                    properties: None,
                    strict_properties: None,
                }],
                edge_types: vec![],
            })
            .await
            .expect("apply pack");

        let kinds = list_node_kinds(State(state.clone()))
            .await
            .expect("list node kinds");
        let widget = kinds
            .items
            .iter()
            .find(|kind| kind.name == "widget")
            .expect("widget kind");
        assert!(!widget.unique_name);
    }

    #[tokio::test]
    async fn direct_kind_creation_can_opt_out_of_unique_names() {
        let state = test_state().await;
        let created = create_node_kind(
            State(state.clone()),
            Json(CreateNodeKindParameters {
                name: "note".to_string(),
                description: "a note".to_string(),
                unique_name: false,
                strict_properties: false,
            }),
        )
        .await
        .expect("create kind")
        .data;
        assert!(!created.unique_name);

        let defaulted: CreateNodeKindParameters =
            serde_json::from_str(r#"{"name":"person","description":"p"}"#)
                .expect("default deserializes");
        assert!(defaulted.unique_name);
    }

    #[tokio::test]
    async fn rejects_double_quotes_in_property_filters() {
        let state = test_state().await;
        let error = list_nodes(
            State(state),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: Some(r#"{"a\"b":1}"#.to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect_err("quote key rejected");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn list_nodes_paginates() {
        let state = test_state().await;
        let kind = seed_kind(&state, "decision").await;
        for index in 0..5 {
            new_node(&state, kind, &format!("p{index}"), "", "", &[]).await;
        }

        let page = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: None,
                limit: Some(2),
                offset: Some(1),
            }),
        )
        .await
        .expect("page");
        assert_eq!(page.data.items.len(), 2);
        assert_eq!(page.data.total, 5);
        assert_eq!(page.data.limit, 2);
        assert_eq!(page.data.offset, 1);

        let clamped = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: None,
                limit: Some(10_000),
                offset: None,
            }),
        )
        .await
        .expect("clamped");
        assert_eq!(
            clamped.data.limit,
            crate::config::PagingConfig::default().max_limit
        );
    }

    #[tokio::test]
    async fn work_views_and_goal_tree_expose_task_status() {
        let state = test_state().await;
        let goal = seed_anchored_goal(&state).await;
        let task = create_task(
            State(state.clone()),
            Json(CreateTaskParameters {
                id: None,
                name: "handler task".to_string(),
                description: String::new(),
                content: String::new(),
                parent: Some(goal),
                depends_on: vec![],
                affects: vec![],
                properties: None,
                tags: Some(vec!["api".to_string()]),
            }),
        )
        .await
        .expect("create task")
        .data;

        let pending = work_pending(
            State(state.clone()),
            Query(WorkParameters {
                goal: Some(goal),
                repo: None,
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("pending");
        assert_eq!(pending.data.items.len(), 1);
        assert_eq!(pending.data.total, 1);
        assert_eq!(pending.data.items[0]["id"], task.id);
        assert_eq!(pending.data.items[0]["status"], "pending");
        assert_eq!(pending.data.items[0]["goal"]["id"], goal);

        let next = work_next(
            State(state.clone()),
            Query(WorkParameters {
                goal: None,
                repo: None,
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("next");
        assert_eq!(next.data.items.len(), 1);

        let compact_work = work_pending(
            State(state.clone()),
            Query(WorkParameters {
                goal: Some(goal),
                repo: None,
                fields: Some("id,status".to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("compact pending")
        .data;
        assert_eq!(
            compact_work.items[0]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["id", "status"]
        );

        let tree = goal_tree(
            State(state.clone()),
            Path(goal),
            Query(PageParameters {
                fields: Some("id,status".to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("tree")
        .data;
        assert_eq!(tree.goal.id, goal);
        assert_eq!(tree.tasks.len(), 1);
        assert_eq!(tree.total, 1);
        assert_eq!(tree.tasks[0]["status"], "pending");

        let error = goal_tree(
            State(state.clone()),
            Path(task.id),
            Query(PageParameters {
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect_err("task is not a goal");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);

        let error = goal_tree(
            State(state.clone()),
            Path(999_999),
            Query(PageParameters {
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect_err("missing goal");
        assert_eq!(error.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn records_a_change_in_one_request() {
        let state = test_state().await;
        let goal = seed_anchored_goal(&state).await;
        let file_kind = state
            .graph_service
            .list_node_kinds()
            .await
            .unwrap()
            .into_iter()
            .find(|kind| kind.name == "file")
            .expect("file kind")
            .id;
        new_node(&state, file_kind, "src/lib.rs", "", "", &[]).await;
        let _task = create_task(
            State(state.clone()),
            Json(CreateTaskParameters {
                id: None,
                name: "change task".to_string(),
                description: String::new(),
                content: String::new(),
                parent: Some(goal),
                depends_on: vec![],
                affects: vec![],
                properties: None,
                tags: None,
            }),
        )
        .await
        .expect("create task")
        .data;

        let change = create_change(
            State(state.clone()),
            Json(CreateChangeParameters {
                commit: "feed42".to_string(),
                title: "handler change".to_string(),
                summary: "details".to_string(),
                tasks: vec![NodeSelectorParameters::Named {
                    kind: "task".to_string(),
                    name: "change task".to_string(),
                }],
                files: vec!["src/lib.rs".to_string()],
                properties: Some(serde_json::json!({"author": "ada"})),
                tags: Some(vec!["API".to_string()]),
            }),
        )
        .await
        .expect("record change")
        .data;
        assert_eq!(change.name, "feed42");
        assert_eq!(change.tags, vec!["api"]);
        assert_eq!(change.properties, serde_json::json!({"author": "ada"}));
        let outgoing = state
            .graph_service
            .neighbors(change.id, crate::model::NeighborDirection::Outgoing, None)
            .await
            .unwrap();
        assert_eq!(outgoing.len(), 2);

        let pending = work_pending(
            State(state.clone()),
            Query(WorkParameters {
                goal: Some(goal),
                repo: None,
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("pending");
        assert!(pending.data.items.is_empty());
    }

    #[tokio::test]
    async fn task_patch_updates_wiring_and_upsert_keeps_parent() {
        let state = test_state().await;
        let goal = seed_anchored_goal(&state).await;
        let first = create_task(
            State(state.clone()),
            Json(CreateTaskParameters {
                id: None,
                name: "wiring first".to_string(),
                description: String::new(),
                content: String::new(),
                parent: Some(goal),
                depends_on: vec![],
                affects: vec![],
                properties: None,
                tags: None,
            }),
        )
        .await
        .expect("first task")
        .data;
        let second = create_task(
            State(state.clone()),
            Json(CreateTaskParameters {
                id: None,
                name: "wiring second".to_string(),
                description: String::new(),
                content: String::new(),
                parent: Some(goal),
                depends_on: vec![],
                affects: vec![],
                properties: None,
                tags: None,
            }),
        )
        .await
        .expect("second task")
        .data;

        let updated = update_task(
            State(state.clone()),
            Path(first.id),
            HeaderMap::new(),
            Json(UpdateTaskParameters {
                name: None,
                description: None,
                content: None,
                properties: None,
                tags: None,
                parent: Some(second.id),
                depends_on: Some(vec![second.id]),
                affects: Some(vec![]),
            }),
        )
        .await
        .expect("patch wiring")
        .data;
        assert_eq!(updated.version, first.version + 1);

        let parents = state
            .graph_service
            .neighbors(
                first.id,
                crate::model::NeighborDirection::Outgoing,
                Some("part_of"),
            )
            .await
            .unwrap();
        assert_eq!(
            parents
                .iter()
                .map(|neighbor| neighbor.node.id)
                .collect::<Vec<_>>(),
            vec![second.id]
        );

        let upserted = create_task(
            State(state.clone()),
            Json(CreateTaskParameters {
                id: Some(first.id),
                name: "wiring first".to_string(),
                description: "changed".to_string(),
                content: String::new(),
                parent: None,
                depends_on: vec![],
                affects: vec![],
                properties: None,
                tags: None,
            }),
        )
        .await
        .expect("upsert without parent")
        .data;
        assert_eq!(upserted.description, "changed");
        let parents = state
            .graph_service
            .neighbors(
                first.id,
                crate::model::NeighborDirection::Outgoing,
                Some("part_of"),
            )
            .await
            .unwrap();
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].node.id, second.id);
    }

    #[tokio::test]
    async fn list_nodes_projects_orders_and_filters_since() {
        let state = test_state().await;
        let kind = seed_kind(&state, "person").await;
        new_node(&state, kind, "beta", "", "", &[]).await;
        new_node(&state, kind, "alpha", "", "", &[]).await;

        let listed = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                ids: None,
                properties: None,
                order: Some("name:desc".to_string()),
                since: None,
                fields: Some("id,name".to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("ordered projection")
        .data;
        assert_eq!(listed.items.len(), 2);
        assert_eq!(listed.items[0]["name"], "beta");
        assert_eq!(listed.items[1]["name"], "alpha");
        assert_eq!(
            listed.items[0]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["id", "name"]
        );

        let future = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                ids: None,
                properties: None,
                order: None,
                since: Some("9999-01-01".to_string()),
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("since filter")
        .data;
        assert!(future.items.is_empty());

        let bad_field = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                ids: None,
                properties: None,
                order: None,
                since: None,
                fields: Some("bogus".to_string()),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect_err("unknown field");
        assert_eq!(bad_field.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(bad_field.field.as_deref(), Some("fields"));

        let bad_order = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                ids: None,
                properties: None,
                order: Some("bogus".to_string()),
                since: None,
                fields: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect_err("unknown order field");
        assert_eq!(bad_order.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn batch_reads_and_export_work() {
        let state = test_state().await;
        let kind = seed_kind(&state, "person").await;
        let first = new_node(&state, kind, "first", "", "", &["Math"]).await;
        let second = new_node(&state, kind, "second", "", "", &[]).await;
        let knows = state
            .graph_service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "knows".to_string(),
                description: String::new(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: None,
                destination_kinds: None,
                updated_at: String::new(),
            })
            .await
            .expect("edge type");
        state
            .graph_service
            .add_edge(&Edge {
                source: first.id,
                destination: second.id,
                edge_type_id: knows.id,
                created_at: String::new(),
            })
            .await
            .expect("edge");

        let batch = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: Some(format!("{},{}", second.id, first.id)),
                properties: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("batch")
        .data;
        assert_eq!(
            batch
                .items
                .iter()
                .map(|node| node["id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![second.id, first.id]
        );
        assert_eq!(batch.total, 2);
        assert_eq!(batch.items[1]["tags"], serde_json::json!(["math"]));
        assert_eq!(batch.items[0]["kind"], "person");

        let named = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: None,
                name: None,
                name_contains: Some("irs".to_string()),
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: None,
                properties: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("name contains")
        .data;
        assert_eq!(named.total, 1);
        assert_eq!(named.items[0]["name"], "first");

        let error = list_nodes(
            State(state.clone()),
            Query(ListNodesParameters {
                kind: Some("person".to_string()),
                name: None,
                name_contains: None,
                scope: None,
                within: None,
                via: None,
                descend: None,
                order: None,
                since: None,
                fields: None,
                ids: Some(first.id.to_string()),
                properties: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect_err("ids cannot be combined with filters");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);

        let edges = list_edges(
            State(state.clone()),
            Query(EdgeListParameters {
                edge_type: Some("knows".to_string()),
                source: None,
                destination: Some(second.id),
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("edges")
        .data;
        assert_eq!(edges.total, 1);
        assert_eq!(edges.items[0].source, first.id);
        assert_eq!(edges.items[0].edge_type, "knows");
        assert_eq!(edges.items[0].version, 1);

        let export = graph_export(State(state.clone()))
            .await
            .expect("export")
            .data;
        assert_eq!(export.node_count, 2);
        assert_eq!(export.edge_count, 1);
        assert!(
            export
                .nodes
                .iter()
                .any(|node| node.name == "first" && node.tags == vec!["math"])
        );
        assert!(export.nodes.iter().all(|node| node.kind == "person"));
        assert_eq!(export.edges[0].edge_type, "knows");
    }

    #[tokio::test]
    async fn update_node_honours_if_match() {
        let state = test_state().await;
        let kind = seed_kind(&state, "person").await;
        let created = new_node(&state, kind, "ada", "", "", &[]).await;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_MATCH,
            axum::http::HeaderValue::from_str(&created.version.to_string()).unwrap(),
        );
        let patch = || UpdateNodeParameters {
            name: None,
            description: Some("changed".to_string()),
            content: None,
            tags: None,
            properties: None,
        };
        let updated = update_node(
            State(state.clone()),
            Path(created.id),
            headers.clone(),
            Json(patch()),
        )
        .await
        .expect("matching version")
        .data;
        assert_eq!(updated.version, created.version + 1);

        let error = update_node(
            State(state.clone()),
            Path(created.id),
            headers,
            Json(patch()),
        )
        .await
        .expect_err("stale version");
        assert_eq!(error.status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(error.code, "precondition_failed");

        let mut malformed = HeaderMap::new();
        malformed.insert(
            header::IF_MATCH,
            axum::http::HeaderValue::from_static("not-a-version"),
        );
        let error = update_node(
            State(state.clone()),
            Path(created.id),
            malformed,
            Json(patch()),
        )
        .await
        .expect_err("malformed If-Match");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error.field.as_deref(), Some("if-match"));
    }

    #[tokio::test]
    async fn query_endpoint_projects_requested_fields() {
        let state = test_state().await;
        let kind = seed_kind(&state, "person").await;
        new_node(&state, kind, "first", "", "", &["Math"]).await;
        new_node(&state, kind, "second", "", "", &[]).await;

        let params: QueryParameters = serde_json::from_value(serde_json::json!({
            "select": "nodes",
            "where": {"kind": "person"},
            "fields": ["id", "kind", "name"],
            "limit": 1
        }))
        .expect("query parameters");
        let data = query(State(state.clone()), Json(params))
            .await
            .expect("query")
            .data;
        assert_eq!(data.total, 2);
        assert_eq!(data.items.len(), 1);
        assert_eq!(
            data.items[0]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["id", "kind", "name"]
        );
    }

    #[tokio::test]
    async fn vocabulary_returns_kinds_types_and_declarations() {
        let state = test_state().await;
        seed_kind(&state, "widget").await;
        state
            .graph_service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "widget_part".to_string(),
                description: "widget -> widget".to_string(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: Some("widget".to_string()),
                destination_kinds: Some("widget".to_string()),
                updated_at: String::new(),
            })
            .await
            .expect("edge type");

        let data = vocabulary(State(state.clone()))
            .await
            .expect("vocabulary")
            .data;
        assert!(data.kinds.iter().any(|kind| kind.name == "widget"));
        assert!(
            data.edge_types
                .iter()
                .any(|edge_type| edge_type.name == "widget_part")
        );
    }

    #[tokio::test]
    async fn update_edge_honours_if_match() {
        let state = test_state().await;
        let kind = seed_kind(&state, "person").await;
        let first = new_node(&state, kind, "first", "", "", &[]).await;
        let second = new_node(&state, kind, "second", "", "", &[]).await;
        let knows = state
            .graph_service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "knows".to_string(),
                description: String::new(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: None,
                destination_kinds: None,
                updated_at: String::new(),
            })
            .await
            .expect("edge type");
        state
            .graph_service
            .add_edge(&Edge {
                source: first.id,
                destination: second.id,
                edge_type_id: knows.id,
                created_at: String::new(),
            })
            .await
            .expect("edge");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, axum::http::HeaderValue::from_static("1"));
        let params = || UpdateEdgeParameters {
            source: first.id,
            destination: second.id,
            edge_type_id: knows.id,
            properties: serde_json::json!({"weight": 1}),
        };
        let status = update_edge(State(state.clone()), headers.clone(), Json(params()))
            .await
            .expect("matching edge version");
        assert_eq!(status, StatusCode::NO_CONTENT);

        let error = update_edge(State(state.clone()), headers, Json(params()))
            .await
            .expect_err("stale edge version");
        assert_eq!(error.status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(error.code, "precondition_failed");
    }

    #[tokio::test]
    async fn creates_edges_from_name_refs() {
        let state = test_state().await;
        let kind = seed_kind(&state, "person").await;
        new_node(&state, kind, "first", "", "", &[]).await;
        new_node(&state, kind, "second", "", "", &[]).await;
        state
            .graph_service
            .create_edge_type(&EdgeType {
                id: 0,
                name: "knows".to_string(),
                description: String::new(),
                resolves: false,
                single_outgoing: false,
                acyclic: false,
                source_kinds: None,
                destination_kinds: None,
                updated_at: String::new(),
            })
            .await
            .expect("edge type");

        let status = add_edge(
            State(state.clone()),
            Json(CreateEdgeParameters {
                source: NodeSelectorParameters::Named {
                    kind: "person".to_string(),
                    name: "first".to_string(),
                },
                destination: NodeSelectorParameters::Named {
                    kind: "person".to_string(),
                    name: "second".to_string(),
                },
                edge_type_id: None,
                edge_type: Some("knows".to_string()),
                properties: None,
            }),
        )
        .await
        .expect("edge by name");
        assert_eq!(status, StatusCode::NO_CONTENT);

        let edges = list_edges(
            State(state.clone()),
            Query(EdgeListParameters {
                edge_type: Some("knows".to_string()),
                source: None,
                destination: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("edges")
        .data;
        assert_eq!(edges.total, 1);
        assert_eq!(edges.items[0].edge_type, "knows");
    }
}
