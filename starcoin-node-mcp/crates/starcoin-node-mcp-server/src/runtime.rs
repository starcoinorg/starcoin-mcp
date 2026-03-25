use anyhow::Result;
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_types::RuntimeConfig;

use crate::server::StarcoinNodeMcpServer;

pub async fn serve_stdio(app: AppContext) -> Result<()> {
    StarcoinNodeMcpServer::new(app).serve_stdio().await
}

pub async fn serve_stdio_with_config(config: RuntimeConfig) -> Result<()> {
    StarcoinNodeMcpServer::bootstrap(config)
        .await?
        .serve_stdio()
        .await
}
