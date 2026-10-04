use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use utoipa::IntoParams;

use crate::dto::wrapper::ApiError;

const MAX_SESSION_ID_LENGTH: usize = 256;

#[derive(IntoParams)]
#[into_params(parameter_in = Header)]
pub struct SessionHeader {
    #[param(rename = "X-Session-Id")]
    pub session_id: String,
}

pub struct SessionId(pub String);

impl<S> FromRequestParts<S> for SessionId
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get("x-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "the X-Session-Id header is required so node access can be attributed to a session".to_string(),
            })?;
        if value.len() > MAX_SESSION_ID_LENGTH {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: format!(
                    "the X-Session-Id header must be at most {MAX_SESSION_ID_LENGTH} characters"
                ),
            });
        }
        Ok(SessionId(value.to_string()))
    }
}
