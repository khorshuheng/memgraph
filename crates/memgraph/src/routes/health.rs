use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::handler::health::*;

pub(crate) fn routes() -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(health_check))
}
