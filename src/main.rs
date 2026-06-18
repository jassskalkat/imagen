mod artifacts;
mod config;
mod cost;
mod error;
mod jobs;
mod mcp;
mod providers;
mod retry;
mod runtime;
mod sandbox;
mod tools;
mod types;

use std::sync::Arc;

use rmcp::ServiceExt;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::config::Provider;
use crate::mcp::ImagenServer;
use crate::providers::azure::AzureProvider;
use crate::providers::openai::OpenAIProvider;
use crate::providers::ImageProvider;
use crate::runtime::state::AppState;
use crate::runtime::worker::Worker;

#[tokio::main]
async fn main() {
    // Initialize tracing to stderr (stdout is used for MCP stdio transport)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    info!("imagen MCP server starting...");

    // Load configuration
    let config = match config::AppConfig::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("Failed to load configuration: {e}");
            std::process::exit(1);
        }
    };

    info!(
        provider = ?config.provider,
        output_dir = %config.output_dir,
        max_concurrent_jobs = config.max_concurrent_jobs,
        "Configuration loaded"
    );

    // Create provider based on config
    let provider: Arc<dyn ImageProvider> = match &config.provider {
        Provider::Azure => {
            let p = AzureProvider::new(&config).unwrap_or_else(|e| {
                tracing::error!("Failed to create Azure provider: {e}");
                std::process::exit(1);
            });
            Arc::new(p)
        }
        Provider::OpenAI => {
            let p = OpenAIProvider::new(&config).unwrap_or_else(|e| {
                tracing::error!("Failed to create OpenAI provider: {e}");
                std::process::exit(1);
            });
            Arc::new(p)
        }
    };

    // Initialize application state
    let state = AppState::new(config, provider);

    // Spawn background worker for job management
    let worker = Worker::new(state.clone());
    worker.spawn();

    // Create and start the MCP server on stdio transport
    let server = ImagenServer::new(state);
    let transport = rmcp::transport::io::stdio();

    info!("Starting MCP server on stdio transport...");

    match server.serve(transport).await {
        Ok(running) => {
            info!("MCP server running, waiting for connections...");
            let _ = running.waiting().await;
            info!("MCP server shut down.");
        }
        Err(e) => {
            tracing::error!("Failed to start MCP server: {e}");
            std::process::exit(1);
        }
    }
}
