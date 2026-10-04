use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{handler::graph::*, state::AppState};

pub(crate) fn routes(app_state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_node_kind, list_node_kinds))
        .routes(routes!(create_edge_type, list_edge_types))
        .routes(routes!(create_node, list_nodes))
        .routes(routes!(create_task))
        .routes(routes!(create_plan))
        .routes(routes!(update_task))
        .routes(routes!(get_node, delete_node))
        .routes(routes!(list_neighbors))
        .routes(routes!(add_edge, remove_edge))
        .routes(routes!(search))
        .routes(routes!(search_segment))
        .routes(routes!(set_node_access_relevance))
        .routes(routes!(graph_health))
        .with_state(app_state)
}
