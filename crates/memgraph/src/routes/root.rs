use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_redoc::{Redoc, Servable};

use crate::{
    dto::wrapper::ApiError, handler::docs, repository::graph::IdempotencyReservation,
    state::AppState,
};

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

async fn idempotency(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() != Method::POST {
        return next.run(request).await;
    }
    let Some(value) = request.headers().get("idempotency-key") else {
        return next.run(request).await;
    };
    let key = match value.to_str() {
        Ok(key) => key.trim().to_string(),
        Err(_) => {
            return ApiError::unprocessable("Idempotency-Key must be ASCII")
                .with_field("idempotency-key")
                .into_response();
        }
    };
    if key.is_empty() || key.len() > 200 || key.chars().any(char::is_control) {
        return ApiError::unprocessable("Idempotency-Key must be 1-200 printable characters")
            .with_field("idempotency-key")
            .into_response();
    }

    let method = request.method().as_str().to_string();
    let path = request.uri().to_string();
    let (parts, body) = request.into_parts();
    let request_body = match to_bytes(body, 2 * 1024 * 1024).await {
        Ok(body) => body,
        Err(_) => {
            return ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request body exceeds 2 MiB",
            )
            .into_response();
        }
    };

    let reservation = match state
        .graph_service
        .idempotency_reserve(&key, &method, &path, &request_body)
        .await
    {
        Ok(reservation) => reservation,
        Err(error) => return ApiError::from(error).into_response(),
    };

    match reservation {
        IdempotencyReservation::Reserved => {
            let request = Request::from_parts(parts, Body::from(request_body));
            let response = next.run(request).await;
            let status = response.status();
            let (response_parts, response_body) = response.into_parts();
            let response_bytes = match to_bytes(response_body, 8 * 1024 * 1024).await {
                Ok(body) => body,
                Err(_) => {
                    let _ = state.graph_service.idempotency_abort(&key).await;
                    return ApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error",
                        "response exceeded the idempotency buffer",
                    )
                    .into_response();
                }
            };
            if status.as_u16() < 500 {
                if let Err(error) = state
                    .graph_service
                    .idempotency_complete(&key, i64::from(status.as_u16()), &response_bytes)
                    .await
                {
                    tracing::error!(%error, "failed to store an idempotent response");
                }
            } else {
                let _ = state.graph_service.idempotency_abort(&key).await;
            }
            Response::from_parts(response_parts, Body::from(response_bytes))
        }
        IdempotencyReservation::Replay { status, body } => Response::builder()
            .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap_or_else(|_| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "failed to replay an idempotent response",
                )
                .into_response()
            }),
        IdempotencyReservation::Conflict => ApiError::new(
            StatusCode::CONFLICT,
            "idempotency_key_reused",
            "this Idempotency-Key was used for a different request",
        )
        .with_hint("use a new key for a different request")
        .into_response(),
        IdempotencyReservation::InProgress => ApiError::new(
            StatusCode::CONFLICT,
            "idempotency_in_progress",
            "a request with this Idempotency-Key is still running",
        )
        .with_hint("retry shortly")
        .with_retryable()
        .into_response(),
    }
}

pub fn routes(app_state: AppState) -> Router {
    let idempotency_state = app_state.clone();
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
        .layer(middleware::from_fn_with_state(
            idempotency_state,
            idempotency,
        ))
}
