#![forbid(unsafe_code)]

use std::io;

use anyhow::Result;
use clap::Parser;
use starcoin_node_mcp_server::serve_stdio_with_config;
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

    serve_stdio_with_config(config).await
}
