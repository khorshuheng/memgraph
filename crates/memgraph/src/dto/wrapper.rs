use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiList<T: Serialize> {
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub status: StatusCode,
    pub items: Vec<T>,
}

impl<T: Serialize> IntoResponse for ApiList<T> {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub retryable: bool,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code: code.to_string(),
            message: message.into(),
            field: None,
            hint: None,
            retryable: false,
        }
    }

    pub fn unprocessable(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
    }

    pub fn with_field(mut self, field: &str) -> Self {
        self.field = Some(field.to_string());
        self
    }

    pub fn with_hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_string());
        self
    }

    pub fn with_retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}
