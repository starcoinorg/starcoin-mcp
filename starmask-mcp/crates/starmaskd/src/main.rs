#![forbid(unsafe_code)]

use std::{io, time::Duration};

use anyhow::Result;
use clap::Parser;
use tokio::time::MissedTickBehavior;
use tracing_subscriber::EnvFilter;

use starmask_core::CoordinatorCommand;
use starmaskd::{
    config::{Cli, Command, RuntimeConfig, ServeArgs},
    coordinator_runtime::spawn_coordinator,
    server::{ServerPolicy, run_unix_server},
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
    config.ensure_runtime_dirs()?;

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(EnvFilter::new(config.log_level.clone()))
        .with_target(false)
        .init();

    let store = SqliteStore::open(&config.database_path)?;
    let coordinator = spawn_coordinator(store, config.coordinator.clone());
    let maintenance_handle = coordinator.clone();
    let maintenance_interval = Duration::from_secs(config.maintenance_interval.as_secs());
    let maintenance_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(maintenance_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(error) = maintenance_handle
                .dispatch(CoordinatorCommand::TickMaintenance)
                .await
            {
                tracing::warn!(%error, "maintenance tick failed");
            }
        }
    });
    let server_policy = ServerPolicy {
        channel: config.channel,
        allowed_extension_ids: config.allowed_extension_ids.clone(),
        native_host_name: config.native_host_name.clone(),
    };

    #[cfg(unix)]
    {
        let result = run_unix_server(&config.socket_path, coordinator, server_policy).await;
        maintenance_task.abort();
        result
    }

    #[cfg(not(unix))]
    {
        let _ = coordinator;
        maintenance_task.abort();
        anyhow::bail!("starmaskd currently supports Unix-domain sockets only")
    }
}
