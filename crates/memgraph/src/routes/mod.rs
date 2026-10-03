use axum::Router;

use crate::state::AppState;

pub mod graph;
pub mod health;
pub mod root;

pub fn router(app_state: AppState) -> Router {
    root::routes(app_state)
}
