#![forbid(unsafe_code)]

use std::io;

use anyhow::Result;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_server::StarcoinNodeMcpServer;
use starcoin_node_mcp_types::{CliArgs, RuntimeConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = CliArgs::parse();
    let config = RuntimeConfig::load(cli)?;

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(EnvFilter::new(config.log_level.clone()))
        .with_target(false)
        .init();

    let app = AppContext::bootstrap(config).await?;
    let service = StarcoinNodeMcpServer::new(app);
    let running_service = service.serve(stdio()).await?;
    let _ = running_service.waiting().await?;
    Ok(())
}
