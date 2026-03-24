use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;

use starmask_core::CoordinatorConfig;
use starmask_types::{Channel, DurationSeconds};

#[derive(Debug, Parser)]
#[command(name = "starmaskd")]
#[command(about = "Local daemon for Starmask MCP integration")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve(ServeArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ServeArgs {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    #[arg(long)]
    pub database_path: Option<PathBuf>,
    #[arg(long)]
    pub log_level: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct FileConfig {
    channel: Option<Channel>,
    socket_path: Option<PathBuf>,
    database_path: Option<PathBuf>,
    log_level: Option<String>,
    default_request_ttl_seconds: Option<u64>,
    min_request_ttl_seconds: Option<u64>,
    max_request_ttl_seconds: Option<u64>,
    delivery_lease_seconds: Option<u64>,
    presentation_lease_seconds: Option<u64>,
    result_retention_seconds: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub channel: Channel,
    pub socket_path: PathBuf,
    pub database_path: PathBuf,
    pub log_level: String,
    pub coordinator: CoordinatorConfig,
}

impl RuntimeConfig {
    pub fn load(args: ServeArgs) -> Result<Self> {
        let file_config = load_file_config(args.config.as_deref())?;
        let channel = read_channel(
            env::var("STARMASKD_CHANNEL").ok(),
            file_config.channel,
            Channel::Development,
        )?;
        let socket_path = args
            .socket_path
            .or_else(|| env::var_os("STARMASKD_SOCKET_PATH").map(PathBuf::from))
            .or(file_config.socket_path)
            .unwrap_or_else(default_socket_path);
        let database_path = args
            .database_path
            .or_else(|| env::var_os("STARMASKD_DB_PATH").map(PathBuf::from))
            .or(file_config.database_path)
            .unwrap_or_else(default_database_path);
        let log_level = args
            .log_level
            .or_else(|| env::var("STARMASKD_LOG_LEVEL").ok())
            .or(file_config.log_level)
            .unwrap_or_else(|| "info".to_owned());

        let coordinator = CoordinatorConfig {
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            socket_scope: "local-user".to_owned(),
            db_schema_version: 1,
            default_request_ttl: DurationSeconds::new(
                env_u64("STARMASKD_DEFAULT_REQUEST_TTL_SECONDS")
                    .or(file_config.default_request_ttl_seconds)
                    .unwrap_or(300),
            ),
            min_request_ttl: DurationSeconds::new(
                env_u64("STARMASKD_MIN_REQUEST_TTL_SECONDS")
                    .or(file_config.min_request_ttl_seconds)
                    .unwrap_or(30),
            ),
            max_request_ttl: DurationSeconds::new(
                env_u64("STARMASKD_MAX_REQUEST_TTL_SECONDS")
                    .or(file_config.max_request_ttl_seconds)
                    .unwrap_or(3600),
            ),
            delivery_lease_ttl: DurationSeconds::new(
                env_u64("STARMASKD_DELIVERY_LEASE_SECONDS")
                    .or(file_config.delivery_lease_seconds)
                    .unwrap_or(30),
            ),
            presentation_lease_ttl: DurationSeconds::new(
                env_u64("STARMASKD_PRESENTATION_LEASE_SECONDS")
                    .or(file_config.presentation_lease_seconds)
                    .unwrap_or(45),
            ),
            result_retention: DurationSeconds::new(
                env_u64("STARMASKD_RESULT_RETENTION_SECONDS")
                    .or(file_config.result_retention_seconds)
                    .unwrap_or(600),
            ),
        };

        validate_paths(&socket_path, &database_path)?;

        Ok(Self {
            channel,
            socket_path,
            database_path,
            log_level,
            coordinator,
        })
    }
}

fn load_file_config(path: Option<&Path>) -> Result<FileConfig> {
    let default_path = default_config_path();
    let Some(path) = path.or(default_path.as_deref()) else {
        return Ok(FileConfig::default());
    };
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("failed to parse config file at {}", path.display()))
}

fn read_channel(
    env_value: Option<String>,
    file_value: Option<Channel>,
    default: Channel,
) -> Result<Channel> {
    match env_value {
        Some(value) => match value.as_str() {
            "development" => Ok(Channel::Development),
            "staging" => Ok(Channel::Staging),
            "production" => Ok(Channel::Production),
            other => bail!("unsupported channel value: {other}"),
        },
        None => Ok(file_value.unwrap_or(default)),
    }
}

fn env_u64(key: &str) -> Option<u64> {
    env::var(key).ok().and_then(|value| value.parse().ok())
}

fn validate_paths(socket_path: &Path, database_path: &Path) -> Result<()> {
    let Some(socket_parent) = socket_path.parent() else {
        bail!("socket path must have a parent directory");
    };
    let Some(database_parent) = database_path.parent() else {
        bail!("database path must have a parent directory");
    };
    fs::create_dir_all(socket_parent)
        .with_context(|| format!("failed to create {}", socket_parent.display()))?;
    fs::create_dir_all(database_parent)
        .with_context(|| format!("failed to create {}", database_parent.display()))?;
    Ok(())
}

fn default_config_path() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    if cfg!(target_os = "macos") {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("StarcoinMCP")
                .join("config.toml"),
        )
    } else {
        Some(
            home.join(".config")
                .join("starcoin-mcp")
                .join("config.toml"),
        )
    }
}

fn default_socket_path() -> PathBuf {
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

fn default_database_path() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("StarcoinMCP")
            .join("starmaskd.sqlite3")
    } else {
        home.join(".local")
            .join("state")
            .join("starcoin-mcp")
            .join("starmaskd.sqlite3")
    }
}
