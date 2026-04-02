#![forbid(unsafe_code)]

use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_types::{
    CallViewFunctionInput, CliArgs, EmptyParams, GetAccountOverviewInput, GetBlockInput,
    GetEventsInput, GetTransactionInput, ListBlocksInput, ListModulesInput, ListResourcesInput,
    PrepareContractCallInput, PreparePublishPackageInput, PrepareTransferInput,
    ResolveFunctionAbiInput, ResolveModuleAbiInput, ResolveStructAbiInput, RuntimeConfig,
    SimulateRawTransactionInput, SubmitSignedTransactionInput, WatchTransactionInput,
};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "starcoin-node-cli")]
#[command(about = "Non-MCP chain CLI for Starcoin transfer workflows")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Call { tool: String },
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime_config = RuntimeConfig::load(CliArgs {
        config: cli.config,
        ..Default::default()
    })?;

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(EnvFilter::new(runtime_config.log_level.clone()))
        .with_target(false)
        .init();

    let app = AppContext::bootstrap(runtime_config).await?;
    match cli.command {
        Command::Call { tool } => call_tool(&app, &tool).await,
    }
}

async fn call_tool(app: &AppContext, tool: &str) -> Result<()> {
    let arguments = read_json_arguments()?;
    match tool {
        "chain_status" => {
            let _: EmptyParams = parse_arguments(arguments)?;
            write_json(&app.chain_status().await?)
        }
        "node_health" => {
            let _: EmptyParams = parse_arguments(arguments)?;
            write_json(&app.node_health().await?)
        }
        "get_block" => {
            let params: GetBlockInput = parse_arguments(arguments)?;
            write_json(&app.get_block(params).await?)
        }
        "list_blocks" => {
            let params: ListBlocksInput = parse_arguments(arguments)?;
            write_json(&app.list_blocks(params).await?)
        }
        "get_transaction" => {
            let params: GetTransactionInput = parse_arguments(arguments)?;
            write_json(&app.get_transaction(params).await?)
        }
        "watch_transaction" => {
            let params: WatchTransactionInput = parse_arguments(arguments)?;
            write_json(&app.watch_transaction(params).await?)
        }
        "get_events" => {
            let params: GetEventsInput = parse_arguments(arguments)?;
            write_json(&app.get_events(params).await?)
        }
        "get_account_overview" => {
            let params: GetAccountOverviewInput = parse_arguments(arguments)?;
            write_json(&app.get_account_overview(params).await?)
        }
        "list_resources" => {
            let params: ListResourcesInput = parse_arguments(arguments)?;
            write_json(&app.list_resources(params).await?)
        }
        "list_modules" => {
            let params: ListModulesInput = parse_arguments(arguments)?;
            write_json(&app.list_modules(params).await?)
        }
        "resolve_function_abi" => {
            let params: ResolveFunctionAbiInput = parse_arguments(arguments)?;
            write_json(&app.resolve_function_abi(params).await?)
        }
        "resolve_struct_abi" => {
            let params: ResolveStructAbiInput = parse_arguments(arguments)?;
            write_json(&app.resolve_struct_abi(params).await?)
        }
        "resolve_module_abi" => {
            let params: ResolveModuleAbiInput = parse_arguments(arguments)?;
            write_json(&app.resolve_module_abi(params).await?)
        }
        "call_view_function" => {
            let params: CallViewFunctionInput = parse_arguments(arguments)?;
            write_json(&app.call_view_function(params).await?)
        }
        "prepare_transfer" => {
            let params: PrepareTransferInput = parse_arguments(arguments)?;
            write_json(&app.prepare_transfer(params).await?)
        }
        "prepare_contract_call" => {
            let params: PrepareContractCallInput = parse_arguments(arguments)?;
            write_json(&app.prepare_contract_call(params).await?)
        }
        "prepare_publish_package" => {
            let params: PreparePublishPackageInput = parse_arguments(arguments)?;
            write_json(&app.prepare_publish_package(params).await?)
        }
        "simulate_raw_transaction" => {
            let params: SimulateRawTransactionInput = parse_arguments(arguments)?;
            write_json(&app.simulate_raw_transaction(params).await?)
        }
        "submit_signed_transaction" => {
            let params: SubmitSignedTransactionInput = parse_arguments(arguments)?;
            write_json(&app.submit_signed_transaction(params).await?)
        }
        other => bail!("unknown tool: {other}"),
    }
}

fn read_json_arguments() -> Result<Value> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .context("failed to read stdin arguments")?;
    if buffer.trim().is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str(&buffer).context("failed to parse stdin JSON arguments")
    }
}

fn parse_arguments<T>(arguments: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(arguments).context("failed to decode command arguments")
}

fn write_json<T>(value: &T) -> Result<()>
where
    T: Serialize,
{
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, value).context("failed to encode JSON output")?;
    handle
        .write_all(b"\n")
        .context("failed to flush JSON output")?;
    Ok(())
}
