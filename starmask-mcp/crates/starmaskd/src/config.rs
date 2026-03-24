use std::{
    collections::BTreeSet,
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
    allowed_extension_ids: Option<Vec<String>>,
    native_host_name: Option<String>,
    socket_path: Option<PathBuf>,
    database_path: Option<PathBuf>,
    log_level: Option<String>,
    maintenance_interval_seconds: Option<u64>,
    default_request_ttl_seconds: Option<u64>,
    min_request_ttl_seconds: Option<u64>,
    max_request_ttl_seconds: Option<u64>,
    delivery_lease_seconds: Option<u64>,
    presentation_lease_seconds: Option<u64>,
    heartbeat_interval_seconds: Option<u64>,
    wallet_offline_after_seconds: Option<u64>,
    result_retention_seconds: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub channel: Channel,
    pub allowed_extension_ids: BTreeSet<String>,
    pub native_host_name: String,
    pub socket_path: PathBuf,
    pub database_path: PathBuf,
    pub log_level: String,
    pub maintenance_interval: DurationSeconds,
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
        let allowed_extension_ids = read_extension_ids(
            env::var("STARMASKD_ALLOWED_EXTENSION_IDS").ok(),
            file_config.allowed_extension_ids.clone(),
        )?;
        validate_allowed_extension_ids(channel, &allowed_extension_ids)?;
        let native_host_name = env::var("STARMASKD_NATIVE_HOST_NAME")
            .ok()
            .or(file_config.native_host_name.clone())
            .unwrap_or_else(|| default_native_host_name(channel));
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
        let maintenance_interval = DurationSeconds::new(
            env_u64("STARMASKD_MAINTENANCE_INTERVAL_SECONDS")
                .or(file_config.maintenance_interval_seconds)
                .unwrap_or(1)
                .max(1),
        );
        let heartbeat_interval = DurationSeconds::new(
            env_u64("STARMASKD_HEARTBEAT_INTERVAL_SECONDS")
                .or(file_config.heartbeat_interval_seconds)
                .unwrap_or(10)
                .max(1),
        );
        let wallet_offline_after = DurationSeconds::new(
            env_u64("STARMASKD_WALLET_OFFLINE_AFTER_SECONDS")
                .or(file_config.wallet_offline_after_seconds)
                .unwrap_or(25),
        );
        if wallet_offline_after.as_secs() <= heartbeat_interval.as_secs() {
            bail!(
                "wallet_offline_after_seconds ({}) must be greater than heartbeat_interval_seconds ({})",
                wallet_offline_after.as_secs(),
                heartbeat_interval.as_secs(),
            );
        }

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
            wallet_offline_after,
            result_retention: DurationSeconds::new(
                env_u64("STARMASKD_RESULT_RETENTION_SECONDS")
                    .or(file_config.result_retention_seconds)
                    .unwrap_or(600),
            ),
        };

        validate_paths(&socket_path, &database_path)?;

        Ok(Self {
            channel,
            allowed_extension_ids,
            native_host_name,
            socket_path,
            database_path,
            log_level,
            maintenance_interval,
            coordinator,
        })
    }

    pub fn ensure_runtime_dirs(&self) -> Result<()> {
        create_parent_dir(&self.socket_path)?;
        create_parent_dir(&self.database_path)?;
        Ok(())
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
    if socket_path.parent().is_none() {
        bail!("socket path must have a parent directory");
    }
    if database_path.parent().is_none() {
        bail!("database path must have a parent directory");
    }
    Ok(())
}

fn create_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("path must have a parent directory");
    };
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    Ok(())
}

fn read_extension_ids(
    env_value: Option<String>,
    file_value: Option<Vec<String>>,
) -> Result<BTreeSet<String>> {
    let raw_values = if let Some(env_value) = env_value {
        env_value
            .split(',')
            .map(str::trim)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        file_value.unwrap_or_default()
    };

    let mut extension_ids = BTreeSet::new();
    for raw_value in raw_values {
        let value = raw_value.trim();
        if value.is_empty() {
            bail!("allowed_extension_ids contains an empty entry");
        }
        extension_ids.insert(value.to_owned());
    }
    Ok(extension_ids)
}

fn validate_allowed_extension_ids(
    channel: Channel,
    allowed_extension_ids: &BTreeSet<String>,
) -> Result<()> {
    if allowed_extension_ids.is_empty() {
        bail!(
            "allowed_extension_ids must be configured for the {} channel",
            channel_name(channel)
        );
    }
    Ok(())
}

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Development => "development",
        Channel::Staging => "staging",
        Channel::Production => "production",
    }
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

pub fn default_database_path() -> PathBuf {
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

pub fn default_native_host_name(channel: Channel) -> String {
    format!("com.starcoin.starmask.{}", channel_name(channel))
}

#[cfg(test)]
mod tests {
    use super::{
        Channel, default_native_host_name, read_extension_ids, validate_allowed_extension_ids,
    };

    #[test]
    fn read_extension_ids_trims_and_deduplicates_env_values() {
        let result = read_extension_ids(Some(" ext-a ,ext-b,ext-a ".to_owned()), None).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains("ext-a"));
        assert!(result.contains("ext-b"));
    }

    #[test]
    fn read_extension_ids_rejects_empty_entries() {
        let error = read_extension_ids(Some("ext-a,,ext-b".to_owned()), None).unwrap_err();
        assert!(error.to_string().contains("empty entry"));
    }

    #[test]
    fn validate_allowed_extension_ids_rejects_empty_allowlist() {
        let error =
            validate_allowed_extension_ids(Channel::Production, &Default::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("allowed_extension_ids must be configured")
        );
    }

    #[test]
    fn native_host_name_defaults_per_channel() {
        assert_eq!(
            default_native_host_name(Channel::Development),
            "com.starcoin.starmask.development"
        );
        assert_eq!(
            default_native_host_name(Channel::Production),
            "com.starcoin.starmask.production"
        );
    }
}
