use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use starmask_local_account_agent::LocalAccountAgent;
use starmaskd::config::{LocalPromptMode, RuntimeConfig, ServeArgs};

#[derive(Debug, Parser)]
#[command(name = "local-account-agent")]
#[command(about = "Local AccountProvider-backed signer agent for Starmask MCP")]
struct Cli {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    backend_id: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = RuntimeConfig::load(ServeArgs {
        config: Some(cli.config),
        socket_path: None,
        database_path: None,
        log_level: None,
    })?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(runtime.log_level.clone()))
        .init();

    let Some(backend) = runtime.find_backend(&cli.backend_id) else {
        bail!("backend {} is not configured", cli.backend_id);
    };
    let Some(config) = backend.as_local_account_dir().cloned() else {
        bail!("backend {} is not a local_account_dir backend", cli.backend_id);
    };
    if config.prompt_mode != LocalPromptMode::TtyPrompt {
        bail!("local-account-agent currently supports only prompt_mode = tty_prompt");
    }

    let mut agent = LocalAccountAgent::new(
        runtime.socket_path,
        std::time::Duration::from_secs(runtime.heartbeat_interval.as_secs()),
        config,
    )?;
    agent.run()
}
