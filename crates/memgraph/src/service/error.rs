use axum::http::StatusCode;
use thiserror::Error;

use crate::{dto::wrapper::ApiError, repository::error::RepositoryError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServiceError {
    #[error("internal error: {0}")]
    InternalError(String),
    #[error("entity not found")]
    EntityNotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("unprocessable entity: {0}")]
    UnprocessableEntity(String),
}

impl From<RepositoryError> for ServiceError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::RowNotFound => ServiceError::EntityNotFound,
            RepositoryError::UniqueConstraintViolation(err) => ServiceError::Conflict(err),
            RepositoryError::ForeignKeyViolation(err) => ServiceError::UnprocessableEntity(err),
            err => ServiceError::InternalError(err.to_string()),
        }
    }
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
            ServiceError::UnprocessableEntity(_) => ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: error.to_string(),
            },
        }
    }
}
