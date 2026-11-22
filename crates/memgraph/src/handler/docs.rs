use axum::{
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::dto::wrapper::ApiSuccess;

pub struct EndpointDoc {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
    pub example: &'static str,
}

pub const ENDPOINTS: &[EndpointDoc] = &[
    EndpointDoc {
        method: "GET",
        path: "/api",
        description: "This index of endpoints and examples.",
        example: "curl $BASE/api",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/health",
        description: "Readiness probe.",
        example: "curl $BASE/api/health",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/vocabulary",
        description: "Node kinds, edge types and declared properties in one call.",
        example: "curl $BASE/api/vocabulary",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/node-kinds",
        description: "List node kinds.",
        example: "curl $BASE/api/node-kinds",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/node-kinds",
        description: "Declare a node kind.",
        example: "curl -X POST $BASE/api/node-kinds -H 'content-type: application/json' -d '{\"name\":\"decision\",\"description\":\"A choice.\",\"unique_name\":true}'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/edge-types",
        description: "List edge types and their contracts.",
        example: "curl $BASE/api/edge-types",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/edge-types",
        description: "Declare an edge type.",
        example: "curl -X POST $BASE/api/edge-types -H 'content-type: application/json' -d '{\"name\":\"documents\",\"description\":\"file -> decision.\",\"source_kinds\":[\"file\"],\"destination_kinds\":[\"decision\"]}'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/pack",
        description: "Apply a vocabulary pack (kinds, edge types, property declarations).",
        example: "curl -X POST $BASE/api/pack -H 'content-type: application/json' -d '{\"node_kinds\":[],\"edge_types\":[]}'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/graph/import",
        description: "Batch-upsert nodes and edges in one transaction.",
        example: "curl -X POST $BASE/api/graph/import -H 'content-type: application/json' -d '{\"nodes\":[{\"ref\":\"a\",\"kind\":\"task\",\"name\":\"example\"}],\"edges\":[]}'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/nodes",
        description: "List or filter nodes by kind, name, name_contains, tags, properties or reach; order with field:dir, filter with since, and project with fields.",
        example: "curl '$BASE/api/nodes?kind=task&order=updated_at:desc&fields=id,kind,name&limit=20'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/nodes",
        description: "Batch-get nodes by ids in request order (max 200).",
        example: "curl '$BASE/api/nodes?ids=120,119'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/nodes",
        description: "Create a node.",
        example: "curl -X POST $BASE/api/nodes -H 'content-type: application/json' -d '{\"kind_id\":3,\"name\":\"ship it\",\"description\":\"\",\"content\":\"\",\"tags\":[\"plan\"]}'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/nodes/{node_id}",
        description: "Read one node with tags and properties.",
        example: "curl $BASE/api/nodes/120",
    },
    EndpointDoc {
        method: "PATCH",
        path: "/api/nodes/{node_id}",
        description: "Update name, description, content, tags or properties.",
        example: "curl -X PATCH $BASE/api/nodes/120 -H 'content-type: application/json' -d '{\"description\":\"updated\"}'",
    },
    EndpointDoc {
        method: "DELETE",
        path: "/api/nodes/{node_id}",
        description: "Delete a node and its edges.",
        example: "curl -X DELETE $BASE/api/nodes/120",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/nodes/{node_id}/tags",
        description: "Read node tags.",
        example: "curl $BASE/api/nodes/120/tags",
    },
    EndpointDoc {
        method: "PUT",
        path: "/api/nodes/{node_id}/tags",
        description: "Replace node tags; an empty list clears them.",
        example: "curl -X PUT $BASE/api/nodes/120/tags -H 'content-type: application/json' -d '{\"tags\":[\"phase:p0\"]}'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/tags",
        description: "List known tags with node counts.",
        example: "curl $BASE/api/tags",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/nodes/{node_id}/neighbors",
        description: "Read incoming, outgoing or both neighbors with edge type names.",
        example: "curl '$BASE/api/nodes/119/neighbors?direction=incoming&edge_type=part_of'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/edges",
        description: "List edges filtered by edge_type, source or destination.",
        example: "curl '$BASE/api/edges?edge_type=belongs_to&source=119'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/edges",
        description: "Create an edge; endpoints accept ids or {kind,name}, type accepts id or name.",
        example: "curl -X POST $BASE/api/edges -H 'content-type: application/json' -d '{\"source\":122,\"destination\":120,\"edge_type\":\"depends_on\"}'",
    },
    EndpointDoc {
        method: "PATCH",
        path: "/api/edges",
        description: "Replace edge properties for an exact triple.",
        example: "curl -X PATCH $BASE/api/edges -H 'content-type: application/json' -d '{\"source\":119,\"destination\":1,\"edge_type_id\":1,\"properties\":{\"note\":\"x\"}}'",
    },
    EndpointDoc {
        method: "DELETE",
        path: "/api/edges",
        description: "Delete an edge by triple.",
        example: "curl -X DELETE '$BASE/api/edges?source=119&destination=1&edge_type_id=1'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/search",
        description: "Full-text search over names, descriptions and content, with optional tags and properties.",
        example: "curl '$BASE/api/search?q=pending%20tasks&limit=10'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/query",
        description: "Read-only JSON query over nodes or edges with structural predicates, ordering, paging and field projection.",
        example: "curl -X POST $BASE/api/query -H 'content-type: application/json' -d '{\"select\":\"nodes\",\"where\":{\"kind\":\"task\",\"not_has_edge\":{\"edge_type\":\"implements\",\"direction\":\"in\"}},\"order\":[{\"field\":\"updated_at\",\"direction\":\"desc\"}],\"fields\":[\"id\",\"kind\",\"name\"],\"limit\":10}'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/tasks",
        description: "Create or upsert a task under a goal or task.",
        example: "curl -X POST $BASE/api/tasks -H 'content-type: application/json' -d '{\"name\":\"new task\",\"description\":\"\",\"content\":\"\",\"parent\":119}'",
    },
    EndpointDoc {
        method: "PATCH",
        path: "/api/tasks/{task_id}",
        description: "Patch a task's fields, tags or properties.",
        example: "curl -X PATCH $BASE/api/tasks/120 -H 'content-type: application/json' -d '{\"tags\":[\"phase:p0\"]}'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/plans",
        description: "Create a goal with a task tree, dependencies and wiring in one transaction.",
        example: "curl -X POST $BASE/api/plans -H 'content-type: application/json' -d '{\"goal\":{\"name\":\"ship\",\"description\":\"\",\"content\":\"\"},\"anchor\":{\"node_id\":1,\"edge_type\":\"belongs_to\"},\"tasks\":[{\"key\":\"a\",\"name\":\"first\",\"description\":\"\",\"content\":\"\"}]}'",
    },
    EndpointDoc {
        method: "POST",
        path: "/api/changes",
        description: "Record a landed change: implements task refs and modifies file refs atomically.",
        example: "curl -X POST $BASE/api/changes -H 'content-type: application/json' -d '{\"commit\":\"abc1234\",\"title\":\"do work\",\"tasks\":[120],\"files\":[]}'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/work/pending",
        description: "All unfinished tasks with done/blocked state and dependencies; fields projects the task shape.",
        example: "curl '$BASE/api/work/pending?repo=1&limit=20'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/work/next",
        description: "Unfinished tasks whose dependencies are all done; fields projects the task shape.",
        example: "curl '$BASE/api/work/next?repo=1'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/goals/{goal_id}/tree",
        description: "Goal plus its transitive part_of task tree with status and implementing changes; fields projects the task shape.",
        example: "curl '$BASE/api/goals/119/tree?limit=100'",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/graph/export",
        description: "Compact node and edge snapshot (capped at 50k nodes / 200k edges).",
        example: "curl $BASE/api/graph/export",
    },
    EndpointDoc {
        method: "GET",
        path: "/api/graph-health",
        description: "Read-only graph shape report: dead kinds, unused edge types, isolated and unanchored work.",
        example: "curl $BASE/api/graph-health",
    },
    EndpointDoc {
        method: "GET",
        path: "/openapi.json",
        description: "Machine-readable OpenAPI document.",
        example: "curl $BASE/openapi.json",
    },
    EndpointDoc {
        method: "GET",
        path: "/llms.txt",
        description: "This document as plain text.",
        example: "curl $BASE/llms.txt",
    },
];

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiEndpointResponse {
    pub method: String,
    pub path: String,
    pub description: String,
    pub example: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiIndexResponse {
    pub name: String,
    pub version: String,
    pub base_url: String,
    pub openapi: String,
    pub llms_txt: String,
    pub endpoints: Vec<ApiEndpointResponse>,
}

#[utoipa::path(
    tag = "Docs",
    get,
    path = "/api",
    responses((status = 200, description = "Endpoint index with one example per route", body = ApiIndexResponse))
)]
pub async fn api_index() -> ApiSuccess<ApiIndexResponse> {
    ApiSuccess {
        status: StatusCode::OK,
        data: ApiIndexResponse {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            base_url: "http://127.0.0.1:7373".to_string(),
            openapi: "/openapi.json".to_string(),
            llms_txt: "/llms.txt".to_string(),
            endpoints: ENDPOINTS
                .iter()
                .map(|endpoint| ApiEndpointResponse {
                    method: endpoint.method.to_string(),
                    path: endpoint.path.to_string(),
                    description: endpoint.description.to_string(),
                    example: endpoint.example.to_string(),
                })
                .collect(),
        },
    }
}

#[utoipa::path(
    tag = "Docs",
    get,
    path = "/llms.txt",
    responses((status = 200, description = "Endpoint examples as plain text"))
)]
pub async fn llms_txt() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        llms_text(),
    )
}

fn llms_text() -> String {
    let mut text = String::from(
        "# memgraph\n\n\
         Rust HTTP graph store over SQLite. Use tags, properties and edges to track \
         goals, tasks, changes and files.\n\n\
         Base URL: `http://127.0.0.1:7373`.\n\n\
         ```bash\n\
         export BASE=http://127.0.0.1:7373\n\
         ```\n\n\
         Machine-readable contract: `GET /openapi.json`. One call vocabulary: \
         `GET /api/vocabulary`. Ids in the examples come from the running graph; \
         discover them with `GET /api/nodes` or `GET /api/vocabulary`.\n\n\
         POST requests accept an `Idempotency-Key` header; retrying the same key and \
         body replays the stored response. PATCH requests accept \
         `If-Match: \"<version>\"` and return 412 when the node changed.\n\n\
         ## Endpoints (one example per route)\n",
    );
    for endpoint in ENDPOINTS {
        text.push_str(&format!(
            "\n### {} {}\n\n{}\n\n```bash\n{}\n```\n",
            endpoint.method, endpoint.path, endpoint.description, endpoint.example
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[tokio::test]
    async fn index_and_llms_txt_cover_every_endpoint() {
        let index = api_index().await;
        assert_eq!(index.data.base_url, "http://127.0.0.1:7373");
        assert_eq!(index.data.endpoints.len(), ENDPOINTS.len());
        assert!(
            index
                .data
                .endpoints
                .iter()
                .any(|endpoint| endpoint.path == "/api/vocabulary")
        );

        let text = llms_text();
        assert!(text.contains("export BASE=http://127.0.0.1:7373"));
        for endpoint in ENDPOINTS {
            assert!(
                text.contains(&format!("{} {}", endpoint.method, endpoint.path)),
                "llms.txt is missing {} {}",
                endpoint.method,
                endpoint.path
            );
            assert!(
                !endpoint.example.is_empty(),
                "{} {} has no example",
                endpoint.method,
                endpoint.path
            );
        }
    }

    #[tokio::test]
    async fn endpoint_table_matches_the_openapi_paths() {
        let doc = crate::routes::root::openapi_doc().await;
        let openapi_paths: HashSet<&str> = doc.paths.paths.keys().map(String::as_str).collect();
        let mut documented: HashSet<&str> =
            ENDPOINTS.iter().map(|endpoint| endpoint.path).collect();
        documented.remove("/openapi.json");
        assert_eq!(documented, openapi_paths);
    }
}
