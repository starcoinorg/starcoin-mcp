#![forbid(unsafe_code)]

use std::{env, io, path::PathBuf, time::Duration};

use anyhow::Result;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

use crate::{daemon_client::LocalDaemonClient, server::StarmaskMcpServer};

mod daemon_client;
mod dto;
mod error_mapping;
mod server;

#[derive(Debug, Parser)]
#[command(name = "starmask-mcp")]
#[command(about = "MCP stdio adapter for Starmask")]
struct Cli {
    #[arg(long)]
    daemon_socket_path: Option<PathBuf>,
    #[arg(long, default_value_t = 5000)]
    rpc_timeout_ms: u64,
    #[arg(long)]
    log_level: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let log_level = cli
        .log_level
        .or_else(|| env::var("STARMASK_MCP_LOG_LEVEL").ok())
        .unwrap_or_else(|| "info".to_owned());

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(EnvFilter::new(log_level))
        .with_target(false)
        .init();

    let daemon_client = LocalDaemonClient::new(
        cli.daemon_socket_path.unwrap_or_else(default_socket_path),
        Duration::from_millis(cli.rpc_timeout_ms),
    );
    let service = StarmaskMcpServer::new(daemon_client);
    let running_service = service.serve(stdio()).await?;
    let _ = running_service.waiting().await?;
    Ok(())
}

fn default_socket_path() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("StarcoinMCP")
            .join("run")
            .join("starmaskd.sock")
    } else {
        home.join(".local")
            .join("state")
            .join("starcoin-mcp")
            .join("starmaskd.sock")
    }
}
