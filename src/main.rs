use glidinghorse::api::grpc::server::AgentOSService;
use glidinghorse::config::settings::Settings;
use glidinghorse::utils::init_logging;
use glidinghorse::api::grpc::server::seapp::se_kernel_service_server::SeKernelServiceServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load().unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load config ({}), using defaults", e);
        Settings::default()
    });

    let _logging_guard = init_logging(&settings.logging);

    if let Err(e) = settings.validate() {
        eprintln!("Configuration error: {}", e);
        eprintln!("Please set AGENT_OS_GATEWAY_API_KEY or configure config.yaml");
        std::process::exit(1);
    }

    std::fs::create_dir_all(&settings.output.directory)?;
    std::fs::create_dir_all(&settings.memory.l0.path)?;

    let addr = settings.api.grpc_addr.parse().unwrap_or_else(|_| {
        "[::1]:50051".parse().expect("default addr parse")
    });
    let agent_os_service = AgentOSService::new(settings)
        .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

    // 异步初始化 BatchAgent 系统（注册 agent、启动触发器）
    agent_os_service.init_batch_system().await;

    // 把已存在的 axum HTTP/SSE 路由(build_router) 与 gRPC 并行挂载，复用同一运行期状态。
    let http_router = agent_os_service.build_http_router();
    let http_port: u16 = std::env::var("AGENT_OS_HTTP_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);
    let http_addr = std::net::SocketAddr::from(([0, 0, 0, 0], http_port));
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(http_addr).await {
            Ok(listener) => {
                tracing::info!("Agent OS HTTP/SSE server starting on {}", http_addr);
                if let Err(e) = axum::serve(listener, http_router).await {
                    tracing::error!("HTTP server error: {}", e);
                }
            }
            Err(e) => tracing::error!("Failed to bind HTTP server on {}: {}", http_addr, e),
        }
    });

    tracing::info!("Agent OS gRPC server starting on {}", addr);

    tonic::transport::Server::builder()
        .add_service(SeKernelServiceServer::new(agent_os_service))
        .serve(addr)
        .await?;

    Ok(())
}
