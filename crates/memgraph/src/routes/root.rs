use axum::{Router, routing::get};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_redoc::{Redoc, Servable};

use crate::state::AppState;

use super::{graph, health};

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
struct ApiDoc;

pub fn routes(app_state: AppState) -> Router {
    let api_router = health::routes().merge(graph::routes(app_state));
    let (app_router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api", api_router)
        .split_for_parts();
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
