use sqlx::error::ErrorKind;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("sql execution failure: {0}")]
    SqlExecutionError(String),
    #[error("row not found")]
    RowNotFound,
    #[error("unable to acquire connection")]
    ConnectionTimeOut,
    #[error("unique constraint violation: {0}")]
    UniqueConstraintViolation(String),
    #[error("foreign key violation: {0}")]
    ForeignKeyViolation(String),
    #[error("not null violation: {0}")]
    NotNullViolation(String),
    #[error("check constraint violation: {0}")]
    CheckViolation(String),
}

impl From<sqlx::Error> for RepositoryError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => RepositoryError::RowNotFound,
            sqlx::Error::PoolTimedOut => RepositoryError::ConnectionTimeOut,
            sqlx::Error::Database(err) => match err.kind() {
                ErrorKind::UniqueViolation => {
                    RepositoryError::UniqueConstraintViolation(err.to_string())
                }
                ErrorKind::ForeignKeyViolation => {
                    RepositoryError::ForeignKeyViolation(err.to_string())
                }
                ErrorKind::NotNullViolation => RepositoryError::NotNullViolation(err.to_string()),
                ErrorKind::CheckViolation => RepositoryError::CheckViolation(err.to_string()),
                _ => RepositoryError::SqlExecutionError(err.to_string()),
            },
            _ => RepositoryError::SqlExecutionError(error.to_string()),
        }
    }
}
