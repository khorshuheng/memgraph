use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize)]
pub struct ApiSuccess<T: Serialize> {
    #[serde(skip_serializing)]
    pub status: StatusCode,
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
    #[serde(rename = "data")]
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

#[derive(Serialize)]
struct ApiErrorEnvelope<'a> {
    error: &'a ApiError,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ApiErrorEnvelope { error: &self })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    #[test]
    fn success_and_list_use_the_data_envelope() {
        let success = serde_json::to_value(ApiSuccess {
            status: StatusCode::OK,
            data: "value",
        })
        .unwrap();
        assert_eq!(success, serde_json::json!({"data": "value"}));

        let list = serde_json::to_value(ApiList {
            status: StatusCode::OK,
            items: vec!["a", "b"],
        })
        .unwrap();
        assert_eq!(list, serde_json::json!({"data": ["a", "b"]}));
    }

    #[tokio::test]
    async fn errors_use_the_error_envelope() {
        let response = ApiError::unprocessable("bad input").into_response();
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "invalid_request");
        assert_eq!(body["error"]["message"], "bad input");
        assert_eq!(body["error"]["retryable"], false);
    }
}
