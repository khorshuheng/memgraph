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
    #[error("service unavailable: {0}")]
    Unavailable(String),
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    #[error("precondition failed: {0}")]
    PreconditionFailed(String),
}

impl From<RepositoryError> for ServiceError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::RowNotFound => ServiceError::EntityNotFound,
            RepositoryError::UniqueConstraintViolation(err) => ServiceError::Conflict(err),
            RepositoryError::ForeignKeyViolation(err) => ServiceError::UnprocessableEntity(err),
            RepositoryError::ContractViolation(err) => ServiceError::UnprocessableEntity(err),
            RepositoryError::Busy(err) => ServiceError::Unavailable(err),
            RepositoryError::LimitExceeded(err) => ServiceError::PayloadTooLarge(err),
            RepositoryError::PreconditionFailed(err) => ServiceError::PreconditionFailed(err),
            err => ServiceError::InternalError(err.to_string()),
        }
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::InternalError(_) => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            ),
            ServiceError::EntityNotFound => {
                ApiError::new(StatusCode::NOT_FOUND, "not_found", error.to_string())
            }
            ServiceError::Conflict(_) => {
                ApiError::new(StatusCode::CONFLICT, "conflict", error.to_string())
            }
            ServiceError::UnprocessableEntity(_) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_request",
                error.to_string(),
            ),
            ServiceError::Unavailable(_) => ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                error.to_string(),
            )
            .with_hint("retry the request")
            .with_retryable(),
            ServiceError::PayloadTooLarge(_) => ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                error.to_string(),
            ),
            ServiceError::PreconditionFailed(_) => ApiError::new(
                StatusCode::PRECONDITION_FAILED,
                "precondition_failed",
                error.to_string(),
            )
            .with_hint("reload the resource and retry with the current version"),
        }
    }
}
