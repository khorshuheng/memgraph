use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Serialize)]
pub struct ApiSuccess<T: Serialize> {
    #[serde(skip_serializing)]
    pub status: StatusCode,
    #[serde(flatten)]
    pub data: T,
}

impl<T: Serialize> IntoResponse for ApiSuccess<T> {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

#[derive(Serialize)]
pub struct ApiError {
    #[serde(skip_serializing)]
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}
