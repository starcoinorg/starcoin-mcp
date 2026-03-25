#![forbid(unsafe_code)]

use std::{env, path::PathBuf};

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};

mod daemon_client;
mod dto;
mod error_mapping;
mod server;

pub use daemon_client::{DaemonClient, LocalDaemonClient};
pub use error_mapping::AdapterError;
pub use server::StarmaskMcpServer;

pub async fn serve_stdio<C>(daemon_client: C) -> Result<()>
where
    C: DaemonClient,
{
    let service = StarmaskMcpServer::new(daemon_client);
    let running_service = service.serve(stdio()).await?;
    let _ = running_service.waiting().await?;
    Ok(())
}

pub fn default_socket_path() -> PathBuf {
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
