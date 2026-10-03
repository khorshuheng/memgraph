use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub enum HealthCheckStatus {
    Pass,
    #[allow(dead_code)]
    Fail,
}

#[derive(Serialize, ToSchema)]
pub struct HealthCheckResponse {
    pub result: HealthCheckStatus,
}
