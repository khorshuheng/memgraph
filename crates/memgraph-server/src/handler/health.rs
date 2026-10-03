use axum::http::StatusCode;

use crate::dto::{
    health::{HealthCheckResponse, HealthCheckStatus},
    wrapper::{ApiError, ApiSuccess},
};

#[utoipa::path(
    tag = "Health",
    get,
    path = "/health",
    responses(
        (status = 200, description = "Health check passed", body = HealthCheckResponse)
    )
)]
pub async fn health_check() -> Result<ApiSuccess<HealthCheckResponse>, ApiError> {
    Ok(ApiSuccess {
        status: StatusCode::OK,
        data: HealthCheckResponse {
            result: HealthCheckStatus::Pass,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_check_passes() {
        assert!(health_check().await.is_ok());
    }
}
