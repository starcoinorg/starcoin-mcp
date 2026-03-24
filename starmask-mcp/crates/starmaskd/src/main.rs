#![forbid(unsafe_code)]

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use starmaskd::{
    config::{Cli, Command, RuntimeConfig, ServeArgs},
    coordinator_runtime::spawn_coordinator,
    server::run_unix_server,
    sqlite_store::SqliteStore,
};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let args = match cli.command.unwrap_or(Command::Serve(ServeArgs {
        config: None,
        socket_path: None,
        database_path: None,
        log_level: None,
    })) {
        Command::Serve(args) => args,
    };
    let config = RuntimeConfig::load(args)?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config.log_level.clone()))
        .with_target(false)
        .init();

    let store = SqliteStore::open(&config.database_path)?;
    let coordinator = spawn_coordinator(store, config.coordinator.clone());

    #[cfg(unix)]
    {
        run_unix_server(&config.socket_path, coordinator).await
    }

    #[cfg(not(unix))]
    {
        let _ = coordinator;
        anyhow::bail!("starmaskd currently supports Unix-domain sockets only")
    }
}
