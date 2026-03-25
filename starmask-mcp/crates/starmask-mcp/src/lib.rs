#![forbid(unsafe_code)]

use std::{env, path::PathBuf};

mod daemon_client;
mod dto;
mod error_mapping;
mod runtime;
mod server;

pub use daemon_client::{DaemonClient, LocalDaemonClient};
pub use error_mapping::AdapterError;
pub use runtime::serve_stdio;
pub use server::StarmaskMcpServer;

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
