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
mod setup;
#[cfg(test)]
mod test_utils;
mod tools;
mod types;

use std::sync::Arc;

use rmcp::ServiceExt;
use tokio_util::sync::CancellationToken;
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
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("setup") => {
            if let Err(err) = setup::run() {
                eprintln!("setup failed: {err}");
                std::process::exit(1);
            }
            return;
        }
        Some("-h") | Some("--help") | Some("help") => {
            println!("Usage: imagen [setup]");
            println!();
            println!("  imagen         Start the MCP server using the local config file");
            println!("  imagen setup   Create or update the local config file");
            return;
        }
        _ => {}
    }

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
    let config = match config::AppConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("Failed to load configuration: {e}");
            eprintln!("Hint: run `imagen setup` once to create the local config file.");
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

    // Create a cancellation token for graceful shutdown
    let cancel_token = CancellationToken::new();

    // Spawn background worker for job management
    let worker = Worker::new(state.clone(), cancel_token.clone());
    let worker_handle = worker.spawn();

    // Create and start the MCP server on stdio transport
    let server = ImagenServer::new(state);
    let transport = rmcp::transport::io::stdio();

    info!("Starting MCP server on stdio transport...");

    let server_result = server.serve(transport).await;
    match server_result {
        Ok(running) => {
            info!("MCP server running, waiting for connections...");

            // Race between the server finishing and a shutdown signal
            tokio::select! {
                result = running.waiting() => {
                    info!("MCP server shut down: {:?}", result);
                }
                _ = shutdown_signal() => {
                    info!("Shutdown signal received, stopping server...");
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to start MCP server: {e}");
            std::process::exit(1);
        }
    }

    // Signal the worker to stop and wait for it to finish
    cancel_token.cancel();
    let _ = worker_handle.await;
    info!("imagen MCP server stopped.");
}

/// Wait for a shutdown signal (Ctrl+C or SIGTERM on Unix).
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
    }
}
