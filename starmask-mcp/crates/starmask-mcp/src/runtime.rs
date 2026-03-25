use anyhow::Result;

use crate::{daemon_client::DaemonClient, server::StarmaskMcpServer};

pub async fn serve_stdio<C>(daemon_client: C) -> Result<()>
where
    C: DaemonClient,
{
    StarmaskMcpServer::new(daemon_client).serve_stdio().await
}
