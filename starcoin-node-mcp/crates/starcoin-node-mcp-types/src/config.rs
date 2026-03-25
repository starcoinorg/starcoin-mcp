use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use clap::Parser;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::{Mode, VmProfile};

#[derive(Debug, Parser, Clone)]
#[command(name = "starcoin-node-mcp")]
#[command(about = "Chain-facing MCP server for Starcoin nodes")]
pub struct CliArgs {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, env = "STARCOIN_NODE_MCP_RPC_ENDPOINT_URL")]
    pub rpc_endpoint_url: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MODE")]
    pub mode: Option<Mode>,
    #[arg(long, env = "STARCOIN_NODE_MCP_VM_PROFILE")]
    pub vm_profile: Option<VmProfile>,
    #[arg(long, env = "STARCOIN_NODE_MCP_EXPECTED_CHAIN_ID")]
    pub expected_chain_id: Option<u8>,
    #[arg(long, env = "STARCOIN_NODE_MCP_EXPECTED_NETWORK")]
    pub expected_network: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_EXPECTED_GENESIS_HASH")]
    pub expected_genesis_hash: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_REQUIRE_GENESIS_HASH_MATCH")]
    pub require_genesis_hash_match: Option<bool>,
    #[arg(long, env = "STARCOIN_NODE_MCP_CONNECT_TIMEOUT_MS")]
    pub connect_timeout_ms: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_REQUEST_TIMEOUT_MS")]
    pub request_timeout_ms: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_STARTUP_PROBE_TIMEOUT_MS")]
    pub startup_probe_timeout_ms: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_RPC_AUTH_TOKEN")]
    pub rpc_auth_token: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_RPC_AUTH_TOKEN_ENV")]
    pub rpc_auth_token_env: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_RPC_HEADERS")]
    pub rpc_headers: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_TLS_SERVER_NAME")]
    pub tls_server_name: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_ALLOWED_RPC_HOSTS")]
    pub allowed_rpc_hosts: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_TLS_PINNED_SPKI_SHA256")]
    pub tls_pinned_spki_sha256: Option<String>,
    #[arg(long, env = "STARCOIN_NODE_MCP_ALLOW_INSECURE_REMOTE_TRANSPORT")]
    pub allow_insecure_remote_transport: Option<bool>,
    #[arg(long, env = "STARCOIN_NODE_MCP_ALLOW_READ_ONLY_CHAIN_AUTODETECT")]
    pub allow_read_only_chain_autodetect: Option<bool>,
    #[arg(long, env = "STARCOIN_NODE_MCP_DEFAULT_EXPIRATION_TTL_SECONDS")]
    pub default_expiration_ttl_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_EXPIRATION_TTL_SECONDS")]
    pub max_expiration_ttl_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_WATCH_POLL_INTERVAL_SECONDS")]
    pub watch_poll_interval_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_WATCH_TIMEOUT_SECONDS")]
    pub watch_timeout_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_HEAD_LAG_SECONDS")]
    pub max_head_lag_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_WARN_HEAD_LAG_SECONDS")]
    pub warn_head_lag_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_ALLOW_SUBMIT_WITHOUT_PRIOR_SIMULATION")]
    pub allow_submit_without_prior_simulation: Option<bool>,
    #[arg(long, env = "STARCOIN_NODE_MCP_CHAIN_STATUS_CACHE_TTL_SECONDS")]
    pub chain_status_cache_ttl_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_ABI_CACHE_TTL_SECONDS")]
    pub abi_cache_ttl_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MODULE_CACHE_MAX_ENTRIES")]
    pub module_cache_max_entries: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_DISABLE_DISK_CACHE")]
    pub disable_disk_cache: Option<bool>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_SUBMIT_BLOCKING_TIMEOUT_SECONDS")]
    pub max_submit_blocking_timeout_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_WATCH_TIMEOUT_SECONDS")]
    pub max_watch_timeout_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MIN_WATCH_POLL_INTERVAL_SECONDS")]
    pub min_watch_poll_interval_seconds: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_LIST_BLOCKS_COUNT")]
    pub max_list_blocks_count: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_EVENTS_LIMIT")]
    pub max_events_limit: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_ACCOUNT_RESOURCE_LIMIT")]
    pub max_account_resource_limit: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_ACCOUNT_MODULE_LIMIT")]
    pub max_account_module_limit: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_LIST_RESOURCES_SIZE")]
    pub max_list_resources_size: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_LIST_MODULES_SIZE")]
    pub max_list_modules_size: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_PUBLISH_PACKAGE_BYTES")]
    pub max_publish_package_bytes: Option<u64>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_CONCURRENT_WATCH_REQUESTS")]
    pub max_concurrent_watch_requests: Option<usize>,
    #[arg(long, env = "STARCOIN_NODE_MCP_MAX_INFLIGHT_EXPENSIVE_REQUESTS")]
    pub max_inflight_expensive_requests: Option<usize>,
    #[arg(long)]
    pub log_level: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub rpc_endpoint_url: Url,
    pub mode: Mode,
    pub vm_profile: VmProfile,
    pub expected_chain_id: Option<u8>,
    pub expected_network: Option<String>,
    pub expected_genesis_hash: Option<String>,
    pub require_genesis_hash_match: bool,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub startup_probe_timeout: Duration,
    pub rpc_auth_token: Option<RedactedString>,
    pub rpc_headers: Vec<(String, RedactedString)>,
    pub tls_server_name: Option<String>,
    pub allowed_rpc_hosts: Vec<String>,
    pub tls_pinned_spki_sha256: Vec<String>,
    pub allow_insecure_remote_transport: bool,
    pub allow_read_only_chain_autodetect: bool,
    pub default_expiration_ttl: Duration,
    pub max_expiration_ttl: Duration,
    pub watch_poll_interval: Duration,
    pub watch_timeout: Duration,
    pub max_head_lag: Duration,
    pub warn_head_lag: Duration,
    pub allow_submit_without_prior_simulation: bool,
    pub chain_status_cache_ttl: Duration,
    pub abi_cache_ttl: Duration,
    pub module_cache_max_entries: u64,
    pub disable_disk_cache: bool,
    pub max_submit_blocking_timeout: Duration,
    pub max_watch_timeout: Duration,
    pub min_watch_poll_interval: Duration,
    pub max_list_blocks_count: u64,
    pub max_events_limit: u64,
    pub max_account_resource_limit: u64,
    pub max_account_module_limit: u64,
    pub max_list_resources_size: u64,
    pub max_list_modules_size: u64,
    pub max_publish_package_bytes: u64,
    pub max_concurrent_watch_requests: usize,
    pub max_inflight_expensive_requests: usize,
    pub config_path: Option<PathBuf>,
    pub log_level: String,
}

#[derive(Clone)]
pub struct RedactedString(String);

impl RedactedString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RedactedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
struct FileConfig {
    rpc_endpoint_url: Option<String>,
    mode: Option<Mode>,
    vm_profile: Option<VmProfile>,
    expected_chain_id: Option<u8>,
    expected_network: Option<String>,
    expected_genesis_hash: Option<String>,
    require_genesis_hash_match: Option<bool>,
    connect_timeout_ms: Option<u64>,
    request_timeout_ms: Option<u64>,
    startup_probe_timeout_ms: Option<u64>,
    rpc_auth_token_env: Option<String>,
    rpc_headers: Option<String>,
    tls_server_name: Option<String>,
    allowed_rpc_hosts: Option<String>,
    tls_pinned_spki_sha256: Option<String>,
    allow_insecure_remote_transport: Option<bool>,
    allow_read_only_chain_autodetect: Option<bool>,
    default_expiration_ttl_seconds: Option<u64>,
    max_expiration_ttl_seconds: Option<u64>,
    watch_poll_interval_seconds: Option<u64>,
    watch_timeout_seconds: Option<u64>,
    max_head_lag_seconds: Option<u64>,
    warn_head_lag_seconds: Option<u64>,
    allow_submit_without_prior_simulation: Option<bool>,
    chain_status_cache_ttl_seconds: Option<u64>,
    abi_cache_ttl_seconds: Option<u64>,
    module_cache_max_entries: Option<u64>,
    disable_disk_cache: Option<bool>,
    max_submit_blocking_timeout_seconds: Option<u64>,
    max_watch_timeout_seconds: Option<u64>,
    min_watch_poll_interval_seconds: Option<u64>,
    max_list_blocks_count: Option<u64>,
    max_events_limit: Option<u64>,
    max_account_resource_limit: Option<u64>,
    max_account_module_limit: Option<u64>,
    max_list_resources_size: Option<u64>,
    max_list_modules_size: Option<u64>,
    max_publish_package_bytes: Option<u64>,
    max_concurrent_watch_requests: Option<usize>,
    max_inflight_expensive_requests: Option<usize>,
    log_level: Option<String>,
}

impl RuntimeConfig {
    pub fn load(cli: CliArgs) -> anyhow::Result<Self> {
        let config_path = resolve_config_path(cli.config.as_deref());
        let file_config = match config_path.as_deref() {
            Some(path) if path.exists() => Some(load_file_config(path)?),
            _ => None,
        }
        .unwrap_or_default();

        let endpoint = cli
            .rpc_endpoint_url
            .or(file_config.rpc_endpoint_url)
            .ok_or_else(|| anyhow!("missing rpc endpoint url"))?;
        let rpc_endpoint_url = Url::parse(&endpoint).context("invalid rpc endpoint url")?;
        let mode = cli.mode.or(file_config.mode).unwrap_or(Mode::ReadOnly);
        let vm_profile = cli
            .vm_profile
            .or(file_config.vm_profile)
            .unwrap_or(VmProfile::Auto);
        let expected_chain_id = cli.expected_chain_id.or(file_config.expected_chain_id);
        let expected_network = cli.expected_network.or(file_config.expected_network);
        let expected_genesis_hash = cli
            .expected_genesis_hash
            .or(file_config.expected_genesis_hash);
        let require_genesis_hash_match = cli
            .require_genesis_hash_match
            .or(file_config.require_genesis_hash_match)
            .unwrap_or(true);
        let allow_insecure_remote_transport = cli
            .allow_insecure_remote_transport
            .or(file_config.allow_insecure_remote_transport)
            .unwrap_or(false);
        let allow_read_only_chain_autodetect = cli
            .allow_read_only_chain_autodetect
            .or(file_config.allow_read_only_chain_autodetect)
            .unwrap_or(false);

        let connect_timeout = duration_ms(
            cli.connect_timeout_ms
                .or(file_config.connect_timeout_ms)
                .unwrap_or(3_000),
        );
        let request_timeout = duration_ms(
            cli.request_timeout_ms
                .or(file_config.request_timeout_ms)
                .unwrap_or(10_000),
        );
        let startup_probe_timeout = duration_ms(
            cli.startup_probe_timeout_ms
                .or(file_config.startup_probe_timeout_ms)
                .unwrap_or(10_000),
        );
        let default_expiration_ttl = duration_secs(
            cli.default_expiration_ttl_seconds
                .or(file_config.default_expiration_ttl_seconds)
                .unwrap_or(600),
        );
        let max_expiration_ttl = duration_secs(
            cli.max_expiration_ttl_seconds
                .or(file_config.max_expiration_ttl_seconds)
                .unwrap_or(3_600),
        );
        let watch_poll_interval = duration_secs(
            cli.watch_poll_interval_seconds
                .or(file_config.watch_poll_interval_seconds)
                .unwrap_or(3),
        );
        let watch_timeout = duration_secs(
            cli.watch_timeout_seconds
                .or(file_config.watch_timeout_seconds)
                .unwrap_or(120),
        );
        let max_head_lag = duration_secs(
            cli.max_head_lag_seconds
                .or(file_config.max_head_lag_seconds)
                .unwrap_or(60),
        );
        let mut warn_head_lag = duration_secs(
            cli.warn_head_lag_seconds
                .or(file_config.warn_head_lag_seconds)
                .unwrap_or(15),
        );
        if warn_head_lag > max_head_lag {
            warn_head_lag = max_head_lag;
        }
        let allow_submit_without_prior_simulation = cli
            .allow_submit_without_prior_simulation
            .or(file_config.allow_submit_without_prior_simulation)
            .unwrap_or(true);
        let chain_status_cache_ttl = duration_secs(
            cli.chain_status_cache_ttl_seconds
                .or(file_config.chain_status_cache_ttl_seconds)
                .unwrap_or(3),
        );
        let abi_cache_ttl = duration_secs(
            cli.abi_cache_ttl_seconds
                .or(file_config.abi_cache_ttl_seconds)
                .unwrap_or(300),
        );
        let module_cache_max_entries = cli
            .module_cache_max_entries
            .or(file_config.module_cache_max_entries)
            .unwrap_or(1_024);
        let disable_disk_cache = cli
            .disable_disk_cache
            .or(file_config.disable_disk_cache)
            .unwrap_or(true);
        let max_submit_blocking_timeout = duration_secs(
            cli.max_submit_blocking_timeout_seconds
                .or(file_config.max_submit_blocking_timeout_seconds)
                .unwrap_or(60),
        );
        let max_watch_timeout = duration_secs(
            cli.max_watch_timeout_seconds
                .or(file_config.max_watch_timeout_seconds)
                .unwrap_or(300),
        );
        let min_watch_poll_interval = duration_secs(
            cli.min_watch_poll_interval_seconds
                .or(file_config.min_watch_poll_interval_seconds)
                .unwrap_or(2),
        );
        let max_list_blocks_count = cli
            .max_list_blocks_count
            .or(file_config.max_list_blocks_count)
            .unwrap_or(100);
        let max_events_limit = cli
            .max_events_limit
            .or(file_config.max_events_limit)
            .unwrap_or(200);
        let max_account_resource_limit = cli
            .max_account_resource_limit
            .or(file_config.max_account_resource_limit)
            .unwrap_or(100);
        let max_account_module_limit = cli
            .max_account_module_limit
            .or(file_config.max_account_module_limit)
            .unwrap_or(50);
        let max_list_resources_size = cli
            .max_list_resources_size
            .or(file_config.max_list_resources_size)
            .unwrap_or(100);
        let max_list_modules_size = cli
            .max_list_modules_size
            .or(file_config.max_list_modules_size)
            .unwrap_or(100);
        let max_publish_package_bytes = cli
            .max_publish_package_bytes
            .or(file_config.max_publish_package_bytes)
            .unwrap_or(524_288);
        let max_concurrent_watch_requests = cli
            .max_concurrent_watch_requests
            .or(file_config.max_concurrent_watch_requests)
            .unwrap_or(8);
        let max_inflight_expensive_requests = cli
            .max_inflight_expensive_requests
            .or(file_config.max_inflight_expensive_requests)
            .unwrap_or(16);
        let tls_server_name = cli.tls_server_name.or(file_config.tls_server_name);
        let allowed_rpc_hosts = split_csv(cli.allowed_rpc_hosts.or(file_config.allowed_rpc_hosts));
        let tls_pinned_spki_sha256 = split_csv(
            cli.tls_pinned_spki_sha256
                .or(file_config.tls_pinned_spki_sha256),
        );
        let rpc_headers = parse_secret_headers(cli.rpc_headers.or(file_config.rpc_headers))?;
        let rpc_auth_token = resolve_rpc_auth_token(
            cli.rpc_auth_token,
            cli.rpc_auth_token_env.or(file_config.rpc_auth_token_env),
        )?;
        let log_level = cli
            .log_level
            .or(file_config.log_level)
            .or_else(|| env::var("STARCOIN_NODE_MCP_LOG_LEVEL").ok())
            .unwrap_or_else(|| "info".to_owned());

        let config = Self {
            rpc_endpoint_url,
            mode,
            vm_profile,
            expected_chain_id,
            expected_network,
            expected_genesis_hash,
            require_genesis_hash_match,
            connect_timeout,
            request_timeout,
            startup_probe_timeout,
            rpc_auth_token,
            rpc_headers,
            tls_server_name,
            allowed_rpc_hosts,
            tls_pinned_spki_sha256,
            allow_insecure_remote_transport,
            allow_read_only_chain_autodetect,
            default_expiration_ttl,
            max_expiration_ttl,
            watch_poll_interval,
            watch_timeout,
            max_head_lag,
            warn_head_lag,
            allow_submit_without_prior_simulation,
            chain_status_cache_ttl,
            abi_cache_ttl,
            module_cache_max_entries,
            disable_disk_cache,
            max_submit_blocking_timeout,
            max_watch_timeout,
            min_watch_poll_interval,
            max_list_blocks_count,
            max_events_limit,
            max_account_resource_limit,
            max_account_module_limit,
            max_list_resources_size,
            max_list_modules_size,
            max_publish_package_bytes,
            max_concurrent_watch_requests,
            max_inflight_expensive_requests,
            config_path,
            log_level,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn is_remote(&self) -> bool {
        is_remote_endpoint(&self.rpc_endpoint_url)
    }

    pub fn auth_token_debug(&self) -> Option<&str> {
        self.rpc_auth_token.as_ref().map(|_| "[redacted]")
    }

    pub fn auth_token_raw(&self) -> Option<&str> {
        self.rpc_auth_token.as_ref().map(RedactedString::expose)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        validate_endpoint(
            &self.rpc_endpoint_url,
            self.allow_insecure_remote_transport,
            &self.allowed_rpc_hosts,
        )?;
        validate_mode_requirements(
            self.mode,
            self.expected_chain_id,
            self.expected_network.as_deref(),
            self.expected_genesis_hash.as_deref(),
            self.require_genesis_hash_match,
            self.allow_read_only_chain_autodetect,
            self.is_remote(),
        )?;
        validate_clamps(
            self.default_expiration_ttl,
            self.max_expiration_ttl,
            self.connect_timeout,
            self.request_timeout,
            self.startup_probe_timeout,
            self.watch_poll_interval,
            self.watch_timeout,
            self.max_submit_blocking_timeout,
            self.max_watch_timeout,
            self.min_watch_poll_interval,
            self.max_head_lag,
            self.warn_head_lag,
            self.max_concurrent_watch_requests,
            self.max_inflight_expensive_requests,
        )
    }
}

fn resolve_config_path(cli_path: Option<&Path>) -> Option<PathBuf> {
    cli_path.map(Path::to_path_buf).or_else(default_config_path)
}

fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(default_config_subdir()).join("node-mcp.toml"))
}

fn default_config_subdir() -> &'static str {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        "StarcoinMCP"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "starcoin-mcp"
    }
}

fn load_file_config(path: &Path) -> anyhow::Result<FileConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("invalid TOML in {}", path.display()))
}

fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_secret_headers(input: Option<String>) -> anyhow::Result<Vec<(String, RedactedString)>> {
    let mut parsed = Vec::new();
    for entry in input.unwrap_or_default().split(',').map(str::trim) {
        if entry.is_empty() {
            continue;
        }
        let (name, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid rpc header entry: {entry}"))?;
        parsed.push((
            name.trim().to_owned(),
            RedactedString::new(value.trim().to_owned()),
        ));
    }
    Ok(parsed)
}

fn resolve_rpc_auth_token(
    direct: Option<String>,
    env_name: Option<String>,
) -> anyhow::Result<Option<RedactedString>> {
    if let Some(token) = direct {
        return Ok(Some(RedactedString::new(token)));
    }
    if let Some(env_name) = env_name {
        let value = env::var(&env_name)
            .with_context(|| format!("failed to read rpc auth token env var {env_name}"))?;
        return Ok(Some(RedactedString::new(value)));
    }
    Ok(None)
}

fn validate_endpoint(
    endpoint: &Url,
    allow_insecure_remote_transport: bool,
    allowed_rpc_hosts: &[String],
) -> anyhow::Result<()> {
    let is_remote = is_remote_endpoint(endpoint);
    let scheme = endpoint.scheme();
    if is_remote && scheme != "https" && !allow_insecure_remote_transport {
        bail!("remote endpoints must use https unless allow_insecure_remote_transport is enabled");
    }
    if !matches!(scheme, "http" | "https") {
        bail!("only http and https rpc endpoints are supported");
    }
    if !allowed_rpc_hosts.is_empty() {
        let host = endpoint
            .host_str()
            .ok_or_else(|| anyhow!("rpc endpoint is missing a host"))?;
        if !allowed_rpc_hosts.iter().any(|allowed| allowed == host) {
            bail!("rpc endpoint host {host} is not allowed by allowed_rpc_hosts");
        }
    }
    Ok(())
}

fn validate_mode_requirements(
    mode: Mode,
    expected_chain_id: Option<u8>,
    expected_network: Option<&str>,
    expected_genesis_hash: Option<&str>,
    require_genesis_hash_match: bool,
    allow_read_only_chain_autodetect: bool,
    is_remote: bool,
) -> anyhow::Result<()> {
    match mode {
        Mode::Transaction => {
            if expected_chain_id.is_none() {
                bail!("transaction mode requires expected_chain_id");
            }
            if expected_network.is_none() {
                bail!("transaction mode requires expected_network");
            }
            if is_remote && require_genesis_hash_match && expected_genesis_hash.is_none() {
                bail!(
                    "remote transaction mode requires expected_genesis_hash when genesis matching is enabled"
                );
            }
        }
        Mode::ReadOnly => {
            let missing_pin = expected_chain_id.is_none() || expected_network.is_none();
            if missing_pin && !allow_read_only_chain_autodetect {
                bail!(
                    "read_only mode requires expected_chain_id and expected_network unless allow_read_only_chain_autodetect is enabled"
                );
            }
        }
    }
    Ok(())
}

fn validate_clamps(
    default_expiration_ttl: Duration,
    max_expiration_ttl: Duration,
    connect_timeout: Duration,
    request_timeout: Duration,
    startup_probe_timeout: Duration,
    watch_poll_interval: Duration,
    watch_timeout: Duration,
    max_submit_blocking_timeout: Duration,
    max_watch_timeout: Duration,
    min_watch_poll_interval: Duration,
    max_head_lag: Duration,
    warn_head_lag: Duration,
    max_concurrent_watch_requests: usize,
    max_inflight_expensive_requests: usize,
) -> anyhow::Result<()> {
    if default_expiration_ttl > max_expiration_ttl {
        bail!("default_expiration_ttl_seconds cannot exceed max_expiration_ttl_seconds");
    }
    for (name, value) in [
        ("connect_timeout_ms", connect_timeout),
        ("request_timeout_ms", request_timeout),
        ("startup_probe_timeout_ms", startup_probe_timeout),
        ("watch_timeout_seconds", watch_timeout),
        (
            "max_submit_blocking_timeout_seconds",
            max_submit_blocking_timeout,
        ),
        ("max_watch_timeout_seconds", max_watch_timeout),
        ("max_head_lag_seconds", max_head_lag),
    ] {
        if value.is_zero() {
            bail!("{name} must be greater than zero");
        }
    }
    if watch_poll_interval.is_zero() {
        bail!("watch_poll_interval_seconds must be greater than zero");
    }
    if min_watch_poll_interval.is_zero() {
        bail!("min_watch_poll_interval_seconds must be greater than zero");
    }
    if warn_head_lag > max_head_lag {
        bail!("warn_head_lag_seconds cannot exceed max_head_lag_seconds");
    }
    if max_concurrent_watch_requests == 0 {
        bail!("max_concurrent_watch_requests must be greater than zero");
    }
    if max_inflight_expensive_requests == 0 {
        bail!("max_inflight_expensive_requests must be greater than zero");
    }
    Ok(())
}

fn duration_ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn duration_secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn is_remote_endpoint(endpoint: &Url) -> bool {
    match endpoint.host_str() {
        Some("localhost") | Some("127.0.0.1") | Some("::1") => false,
        Some(host) if host.ends_with(".local") => false,
        Some(_) => true,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use schemars::schema_for;
    use serde_json::Value;

    use super::{CliArgs, FileConfig, RedactedString, RuntimeConfig};
    use crate::domain::{Mode, VmProfile};

    #[test]
    fn transaction_mode_requires_chain_pins() {
        let cli = CliArgs::parse_from([
            "starcoin-node-mcp",
            "--rpc-endpoint-url",
            "http://127.0.0.1:9850",
            "--mode",
            "transaction",
        ]);
        let error = super::RuntimeConfig::load(cli).expect_err("missing chain pins should fail");
        assert!(error.to_string().contains("expected_chain_id"));
    }

    #[test]
    fn read_only_allows_autodetect_override() {
        let cli = CliArgs::parse_from([
            "starcoin-node-mcp",
            "--rpc-endpoint-url",
            "http://127.0.0.1:9850",
            "--mode",
            "read_only",
            "--allow-read-only-chain-autodetect",
            "true",
        ]);
        let config = super::RuntimeConfig::load(cli).expect("autodetect override should work");
        assert_eq!(config.mode, Mode::ReadOnly);
        assert!(config.allow_read_only_chain_autodetect);
    }

    #[test]
    fn mode_and_vm_profile_accept_snake_case_values() {
        let cli = CliArgs::parse_from([
            "starcoin-node-mcp",
            "--rpc-endpoint-url",
            "http://127.0.0.1:9850",
            "--mode",
            "read_only",
            "--vm-profile",
            "vm2_only",
            "--allow-read-only-chain-autodetect",
            "true",
        ]);
        let config = super::RuntimeConfig::load(cli).expect("snake_case enum values should parse");
        assert_eq!(config.mode, Mode::ReadOnly);
        assert_eq!(config.vm_profile, VmProfile::Vm2Only);
    }

    #[test]
    fn mode_and_vm_profile_accept_kebab_case_aliases() {
        let cli = CliArgs::parse_from([
            "starcoin-node-mcp",
            "--rpc-endpoint-url",
            "http://127.0.0.1:9850",
            "--mode",
            "read-only",
            "--vm-profile",
            "legacy-compatible",
            "--allow-read-only-chain-autodetect",
            "true",
        ]);
        let config = super::RuntimeConfig::load(cli).expect("kebab-case enum aliases should parse");
        assert_eq!(config.mode, Mode::ReadOnly);
        assert_eq!(config.vm_profile, VmProfile::LegacyCompatible);
    }

    #[test]
    fn auth_token_debug_stays_redacted_while_raw_accessor_exposes_value() {
        let config = RuntimeConfig {
            rpc_auth_token: Some(RedactedString::new("secret-token".to_owned())),
            ..RuntimeConfig::load(CliArgs::parse_from([
                "starcoin-node-mcp",
                "--rpc-endpoint-url",
                "http://127.0.0.1:9850",
                "--mode",
                "read_only",
                "--allow-read-only-chain-autodetect",
                "true",
            ]))
            .expect("baseline config should load")
        };
        assert_eq!(config.auth_token_debug(), Some("[redacted]"));
        assert_eq!(config.auth_token_raw(), Some("secret-token"));
    }

    #[test]
    fn file_config_toml_roundtrip_preserves_fields() {
        let config = FileConfig {
            rpc_endpoint_url: Some("https://barnard.example.com".to_owned()),
            mode: Some(Mode::Transaction),
            vm_profile: Some(VmProfile::LegacyCompatible),
            expected_chain_id: Some(251),
            expected_network: Some("barnard".to_owned()),
            expected_genesis_hash: Some("0xabc".to_owned()),
            require_genesis_hash_match: Some(true),
            connect_timeout_ms: Some(1_000),
            request_timeout_ms: Some(5_000),
            startup_probe_timeout_ms: Some(3_000),
            rpc_auth_token_env: Some("STARCOIN_TOKEN".to_owned()),
            rpc_headers: Some("x-api-key=secret".to_owned()),
            tls_server_name: Some("barnard.example.com".to_owned()),
            allowed_rpc_hosts: Some("barnard.example.com".to_owned()),
            tls_pinned_spki_sha256: Some(
                "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
            ),
            allow_insecure_remote_transport: Some(false),
            allow_read_only_chain_autodetect: Some(false),
            default_expiration_ttl_seconds: Some(600),
            max_expiration_ttl_seconds: Some(3_600),
            watch_poll_interval_seconds: Some(3),
            watch_timeout_seconds: Some(120),
            max_head_lag_seconds: Some(60),
            warn_head_lag_seconds: Some(15),
            allow_submit_without_prior_simulation: Some(false),
            chain_status_cache_ttl_seconds: Some(30),
            abi_cache_ttl_seconds: Some(300),
            module_cache_max_entries: Some(128),
            disable_disk_cache: Some(true),
            max_submit_blocking_timeout_seconds: Some(60),
            max_watch_timeout_seconds: Some(300),
            min_watch_poll_interval_seconds: Some(2),
            max_list_blocks_count: Some(100),
            max_events_limit: Some(200),
            max_account_resource_limit: Some(100),
            max_account_module_limit: Some(50),
            max_list_resources_size: Some(100),
            max_list_modules_size: Some(100),
            max_publish_package_bytes: Some(524_288),
            max_concurrent_watch_requests: Some(8),
            max_inflight_expensive_requests: Some(16),
            log_level: Some("debug".to_owned()),
        };
        let toml_text = toml::to_string(&config).expect("file config should serialize to TOML");
        let roundtrip: FileConfig =
            toml::from_str(&toml_text).expect("serialized TOML should deserialize");
        assert_eq!(roundtrip, config);
    }

    #[test]
    fn file_config_schema_exposes_expected_properties() {
        let schema = schema_for!(FileConfig);
        let schema_json = serde_json::to_value(schema).expect("schema should serialize");
        for field in [
            "rpc_endpoint_url",
            "mode",
            "vm_profile",
            "expected_chain_id",
            "rpc_auth_token_env",
            "max_publish_package_bytes",
        ] {
            assert!(
                schema_json
                    .pointer(&format!("/properties/{field}"))
                    .is_some(),
                "schema should contain property {field}: {schema_json}",
            );
        }
        let mode_schema = schema_json
            .pointer("/$defs/Mode")
            .or_else(|| schema_json.pointer("/definitions/Mode"))
            .expect("schema should include a Mode definition");
        assert_eq!(
            mode_schema.pointer("/type"),
            Some(&Value::String("string".to_owned()))
        );
        assert!(
            mode_schema.pointer("/enum").is_some(),
            "Mode schema should expose enum values: {mode_schema}"
        );
    }
}
