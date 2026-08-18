mod agent;
mod api;
mod config;
mod graph;
mod grpc;
mod sandbox;
mod server;
mod sync;

use clap::{Parser, Subcommand};
use config::Config;

#[derive(Parser)]
#[command(name = "agentos-daemon", version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Daemon(DaemonAction),
    Register,
}

#[derive(Subcommand)]
enum DaemonAction {
    Start,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon(DaemonAction::Start) => {
            let config = Config::load()?;
            tracing::info!("config loaded from config.yaml");
            server::start_server(config).await?;
        }
        Commands::Register => {
            let _config = Config::load()?;
            tracing::info!("registering with center (not yet implemented)");
        }
    }

    Ok(())
}