use agent_os::api::grpc::server::AgentOSService;
use agent_os::config::settings::Settings;
use agent_os::utils::init_logging;
use agent_os::api::grpc::server::seapp::se_kernel_service_server::SeKernelServiceServer;

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

    let addr = "[::1]:50051".parse()?;
    let agent_os_service = AgentOSService::new(settings)
        .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

    tracing::info!("Agent OS gRPC server starting on {}", addr);

    tonic::transport::Server::builder()
        .add_service(SeKernelServiceServer::new(agent_os_service))
        .serve(addr)
        .await?;

    Ok(())
}
