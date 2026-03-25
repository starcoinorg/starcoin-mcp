#![forbid(unsafe_code)]

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_types::RuntimeConfig;

mod error_mapping;
pub mod server;

pub use error_mapping::AdapterError;
pub use server::StarcoinNodeMcpServer;

pub async fn serve_stdio(app: AppContext) -> Result<()> {
    let service = StarcoinNodeMcpServer::new(app);
    let running_service = service.serve(stdio()).await?;
    let _ = running_service.waiting().await?;
    Ok(())
}

pub async fn serve_stdio_with_config(config: RuntimeConfig) -> Result<()> {
    let app = AppContext::bootstrap(config).await?;
    serve_stdio(app).await
}
