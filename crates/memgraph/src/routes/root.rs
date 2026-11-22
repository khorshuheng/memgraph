use axum::{Router, routing::get};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_redoc::{Redoc, Servable};

use crate::state::AppState;

use super::{graph, health};
use crate::handler::docs;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "memgraph",
        license(identifier = "MIT"),
    ),
    servers(
        (url = "http://localhost:7373"),
    ),
)]
pub(crate) struct ApiDoc;

fn build_router(app_state: AppState) -> (Router, utoipa::openapi::OpenApi) {
    let api_router = health::routes().merge(graph::routes(app_state));
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(docs::api_index))
        .routes(routes!(docs::llms_txt))
        .nest("/api", api_router)
        .split_for_parts()
}

#[cfg(test)]
pub(crate) async fn openapi_doc() -> utoipa::openapi::OpenApi {
    use crate::config::{AppConfig, DatabaseConfig, PagingConfig, ServerConfig};
    let state = AppState::new(&AppConfig {
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
    .await;
    build_router(state).1
}

pub fn routes(app_state: AppState) -> Router {
    let (app_router, api) = build_router(app_state);
    let api_clone = api.clone();
    let api_json_router = Router::new().route(
        "/openapi.json",
        get(move || async { axum::response::Json(api_clone) }),
    );
    app_router
        .merge(Redoc::with_url("/redoc", api))
        .merge(api_json_router)
        .layer(TraceLayer::new_for_http())
}
