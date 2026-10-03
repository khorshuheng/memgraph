use memgraph::{config::AppConfig, routes, state::AppState};
use tokio::signal;
use tracing::info;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();
    let app_config = AppConfig::new().expect("error parsing configuration");
    let app_state = AppState::new(&app_config).await;
    let app_routes = routes::router(app_state);
    let listener_address = app_config.server.listener_address();
    let listener = tokio::net::TcpListener::bind(&listener_address)
        .await
        .expect("error binding app server listener");
    info!("memgraph listening on {}", listener_address);
    axum::serve(listener, app_routes)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("shutting down");
}
