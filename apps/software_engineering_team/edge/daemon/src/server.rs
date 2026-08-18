use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::grpc::KernelClient;

pub struct AppState {
    pub config: Config,
    pub kernel: Mutex<Option<KernelClient>>,
}

pub async fn start_server(config: Config) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;

    if config.kernel.is_enabled() {
        tracing::info!(
            "Kernel gRPC target: {} (lazy connect on first request)",
            config.kernel.target
        );
    }

    let state = Arc::new(AppState {
        config,
        kernel: Mutex::new(None),
    });

    let app = Router::new()
        .nest("/api", crate::api::routes())
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    tracing::info!("daemon server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}