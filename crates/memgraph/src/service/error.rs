use axum::http::StatusCode;
use thiserror::Error;

use crate::dto::wrapper::ApiError;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServiceError {
    #[error("internal error: {0}")]
    InternalError(String),
    #[error("entity not found")]
    EntityNotFound,
    #[error("conflict: {0}")]
    Conflict(String),
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::InternalError(_) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            },
            ServiceError::EntityNotFound => ApiError {
                status: StatusCode::NOT_FOUND,
                message: error.to_string(),
            },
            ServiceError::Conflict(_) => ApiError {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            },
        }
    }
}
