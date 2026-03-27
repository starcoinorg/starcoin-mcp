use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
#[cfg(unix)]
use nix::unistd::Uid;
use serde::Deserialize;
use walkdir::WalkDir;

use starmask_core::CoordinatorConfig;
use starmask_types::{ApprovalSurface, BackendKind, Channel, DurationSeconds, WalletCapability};

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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LocalPromptMode {
    TtyPrompt,
    DesktopPrompt,
}

impl LocalPromptMode {
    pub fn approval_surface(self) -> ApprovalSurface {
        match self {
            Self::TtyPrompt => ApprovalSurface::TtyPrompt,
            Self::DesktopPrompt => ApprovalSurface::DesktopPrompt,
        }
    }
}

#[derive(Clone, Debug)]
struct CommonBackendConfig {
    backend_id: String,
    instance_label: String,
    approval_surface: ApprovalSurface,
}

impl CommonBackendConfig {
    fn new(
        backend_id: impl Into<String>,
        instance_label: impl Into<String>,
        approval_surface: ApprovalSurface,
    ) -> Self {
        Self {
            backend_id: backend_id.into(),
            instance_label: instance_label.into(),
            approval_surface,
        }
    }

    fn backend_id(&self) -> &str {
        &self.backend_id
    }

    fn instance_label(&self) -> &str {
        &self.instance_label
    }

    fn approval_surface(&self) -> ApprovalSurface {
        self.approval_surface
    }
}

#[derive(Clone, Debug)]
pub struct StarmaskExtensionBackendConfig {
    common: CommonBackendConfig,
    allowed_extension_ids: BTreeSet<String>,
    native_host_name: String,
    profile_hint: Option<String>,
}

impl StarmaskExtensionBackendConfig {
    pub fn new(
        backend_id: impl Into<String>,
        instance_label: impl Into<String>,
        approval_surface: ApprovalSurface,
        allowed_extension_ids: BTreeSet<String>,
        native_host_name: impl Into<String>,
        profile_hint: Option<String>,
    ) -> Self {
        Self {
            common: CommonBackendConfig::new(backend_id, instance_label, approval_surface),
            allowed_extension_ids,
            native_host_name: native_host_name.into(),
            profile_hint,
        }
    }

    pub fn backend_id(&self) -> &str {
        self.common.backend_id()
    }

    pub fn instance_label(&self) -> &str {
        self.common.instance_label()
    }

    pub fn approval_surface(&self) -> ApprovalSurface {
        self.common.approval_surface()
    }

    pub fn allowed_extension_ids(&self) -> &BTreeSet<String> {
        &self.allowed_extension_ids
    }

    pub fn native_host_name(&self) -> &str {
        &self.native_host_name
    }

    pub fn profile_hint(&self) -> Option<&str> {
        self.profile_hint.as_deref()
    }
}

#[derive(Clone, Debug)]
pub struct LocalAccountDirBackendConfig {
    common: CommonBackendConfig,
    account_dir: PathBuf,
    prompt_mode: LocalPromptMode,
    chain_id: u8,
    unlock_cache_ttl: DurationSeconds,
    allow_read_only_accounts: bool,
    require_strict_permissions: bool,
}

impl LocalAccountDirBackendConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend_id: impl Into<String>,
        instance_label: impl Into<String>,
        approval_surface: ApprovalSurface,
        account_dir: PathBuf,
        prompt_mode: LocalPromptMode,
        chain_id: u8,
        unlock_cache_ttl: DurationSeconds,
        allow_read_only_accounts: bool,
        require_strict_permissions: bool,
    ) -> Self {
        Self {
            common: CommonBackendConfig::new(backend_id, instance_label, approval_surface),
            account_dir,
            prompt_mode,
            chain_id,
            unlock_cache_ttl,
            allow_read_only_accounts,
            require_strict_permissions,
        }
    }

    pub fn backend_id(&self) -> &str {
        self.common.backend_id()
    }

    pub fn instance_label(&self) -> &str {
        self.common.instance_label()
    }

    pub fn approval_surface(&self) -> ApprovalSurface {
        self.common.approval_surface()
    }

    pub fn account_dir(&self) -> &Path {
        &self.account_dir
    }

    pub fn prompt_mode(&self) -> LocalPromptMode {
        self.prompt_mode
    }

    pub fn chain_id(&self) -> u8 {
        self.chain_id
    }

    pub fn unlock_cache_ttl(&self) -> DurationSeconds {
        self.unlock_cache_ttl
    }

    pub fn allow_read_only_accounts(&self) -> bool {
        self.allow_read_only_accounts
    }

    pub fn require_strict_permissions(&self) -> bool {
        self.require_strict_permissions
    }
}

#[derive(Clone, Debug)]
pub enum WalletBackendConfig {
    StarmaskExtension(StarmaskExtensionBackendConfig),
    LocalAccountDir(LocalAccountDirBackendConfig),
}

impl WalletBackendConfig {
    pub fn backend_id(&self) -> &str {
        match self {
            Self::StarmaskExtension(config) => config.backend_id(),
            Self::LocalAccountDir(config) => config.backend_id(),
        }
    }

    pub fn backend_kind(&self) -> BackendKind {
        match self {
            Self::StarmaskExtension(_) => BackendKind::StarmaskExtension,
            Self::LocalAccountDir(_) => BackendKind::LocalAccountDir,
        }
    }

    pub fn instance_label(&self) -> &str {
        match self {
            Self::StarmaskExtension(config) => config.instance_label(),
            Self::LocalAccountDir(config) => config.instance_label(),
        }
    }

    pub fn approval_surface(&self) -> ApprovalSurface {
        match self {
            Self::StarmaskExtension(config) => config.approval_surface(),
            Self::LocalAccountDir(config) => config.approval_surface(),
        }
    }

    pub fn allowed_capabilities(&self) -> &'static [WalletCapability] {
        match self {
            Self::StarmaskExtension(_) => &[
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
            Self::LocalAccountDir(_) => &[
                WalletCapability::Unlock,
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
        }
    }

    pub fn as_extension(&self) -> Option<&StarmaskExtensionBackendConfig> {
        match self {
            Self::StarmaskExtension(config) => Some(config),
            Self::LocalAccountDir(_) => None,
        }
    }

    pub fn as_local_account_dir(&self) -> Option<&LocalAccountDirBackendConfig> {
        match self {
            Self::StarmaskExtension(_) => None,
            Self::LocalAccountDir(config) => Some(config),
        }
    }
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
    wallet_backends: Option<Vec<FileWalletBackendConfig>>,
}

#[derive(Clone, Debug, Deserialize)]
struct FileWalletBackendConfig {
    backend_id: String,
    backend_kind: BackendKind,
    #[serde(default = "default_enabled")]
    enabled: bool,
    instance_label: String,
    approval_surface: ApprovalSurface,
    allowed_extension_ids: Option<Vec<String>>,
    native_host_name: Option<String>,
    profile_hint: Option<String>,
    account_dir: Option<PathBuf>,
    prompt_mode: Option<LocalPromptMode>,
    chain_id: Option<u8>,
    unlock_cache_ttl_seconds: Option<u64>,
    allow_read_only_accounts: Option<bool>,
    require_strict_permissions: Option<bool>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    channel: Channel,
    socket_path: PathBuf,
    database_path: PathBuf,
    log_level: String,
    maintenance_interval: DurationSeconds,
    heartbeat_interval: DurationSeconds,
    coordinator: CoordinatorConfig,
    wallet_backends: Vec<WalletBackendConfig>,
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
            .or(file_config.socket_path.clone())
            .unwrap_or_else(default_socket_path);
        let database_path = args
            .database_path
            .or_else(|| env::var_os("STARMASKD_DB_PATH").map(PathBuf::from))
            .or(file_config.database_path.clone())
            .unwrap_or_else(default_database_path);
        let log_level = args
            .log_level
            .or_else(|| env::var("STARMASKD_LOG_LEVEL").ok())
            .or(file_config.log_level.clone())
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

        let wallet_backends = build_wallet_backends(channel, &file_config)?;
        if wallet_backends.is_empty() {
            bail!("at least one enabled wallet backend must be configured");
        }

        let coordinator = CoordinatorConfig {
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            socket_scope: "local-user".to_owned(),
            db_schema_version: 2,
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
            socket_path,
            database_path,
            log_level,
            maintenance_interval,
            heartbeat_interval,
            coordinator,
            wallet_backends,
        })
    }

    pub fn ensure_runtime_dirs(&self) -> Result<()> {
        create_parent_dir(&self.socket_path)?;
        create_parent_dir(&self.database_path)?;
        Ok(())
    }

    pub fn channel(&self) -> Channel {
        self.channel
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn log_level(&self) -> &str {
        &self.log_level
    }

    pub fn maintenance_interval(&self) -> DurationSeconds {
        self.maintenance_interval
    }

    pub fn heartbeat_interval(&self) -> DurationSeconds {
        self.heartbeat_interval
    }

    pub fn coordinator(&self) -> &CoordinatorConfig {
        &self.coordinator
    }

    pub fn wallet_backends(&self) -> &[WalletBackendConfig] {
        &self.wallet_backends
    }

    pub fn find_backend(&self, backend_id: &str) -> Option<&WalletBackendConfig> {
        self.wallet_backends
            .iter()
            .find(|backend| backend.backend_id() == backend_id)
    }
}

fn build_wallet_backends(
    channel: Channel,
    file_config: &FileConfig,
) -> Result<Vec<WalletBackendConfig>> {
    build_wallet_backends_with_legacy_env(
        channel,
        file_config,
        env::var("STARMASKD_ALLOWED_EXTENSION_IDS").ok(),
        env::var("STARMASKD_NATIVE_HOST_NAME").ok(),
    )
}

fn build_wallet_backends_with_legacy_env(
    channel: Channel,
    file_config: &FileConfig,
    legacy_allowed_extension_ids_env: Option<String>,
    legacy_native_host_name_env: Option<String>,
) -> Result<Vec<WalletBackendConfig>> {
    if let Some(file_backends) = &file_config.wallet_backends {
        if file_config.allowed_extension_ids.is_some()
            || file_config.native_host_name.is_some()
            || legacy_allowed_extension_ids_env.is_some()
            || legacy_native_host_name_env.is_some()
        {
            bail!("legacy extension settings are not allowed when wallet_backends is configured");
        }
        build_phase2_backends(channel, file_backends)
    } else {
        let allowed_extension_ids = read_extension_ids(
            legacy_allowed_extension_ids_env,
            file_config.allowed_extension_ids.clone(),
        )?;
        validate_allowed_extension_ids(channel, &allowed_extension_ids)?;
        let native_host_name = legacy_native_host_name_env
            .or(file_config.native_host_name.clone())
            .unwrap_or_else(|| default_native_host_name(channel));

        Ok(vec![WalletBackendConfig::StarmaskExtension(
            StarmaskExtensionBackendConfig::new(
                "browser-default",
                "Browser Default",
                ApprovalSurface::BrowserUi,
                allowed_extension_ids,
                native_host_name,
                None,
            ),
        )])
    }
}

fn build_phase2_backends(
    channel: Channel,
    file_backends: &[FileWalletBackendConfig],
) -> Result<Vec<WalletBackendConfig>> {
    let mut seen_backend_ids = BTreeSet::new();
    let mut seen_extension_ids = BTreeSet::new();
    let mut enabled_backends = Vec::new();

    for backend in file_backends {
        let backend_id = backend.backend_id.trim();
        if backend_id.is_empty() {
            bail!("backend_id cannot be empty");
        }
        if !seen_backend_ids.insert(backend_id.to_owned()) {
            bail!("duplicate backend_id configured: {backend_id}");
        }
        if backend.backend_kind == BackendKind::PrivateKeyDev {
            bail!("backend_kind private_key_dev is reserved for a future phase");
        }

        if !backend.enabled {
            continue;
        }

        let instance_label = read_non_empty_string("instance_label", &backend.instance_label)?;
        let approval_surface = backend.approval_surface;

        let parsed = match backend.backend_kind {
            BackendKind::StarmaskExtension => {
                if approval_surface != ApprovalSurface::BrowserUi {
                    bail!(
                        "backend {} must use approval_surface = browser_ui",
                        backend_id
                    );
                }
                let allowed_extension_ids =
                    read_extension_ids(None, backend.allowed_extension_ids.clone())?;
                validate_allowed_extension_ids(channel, &allowed_extension_ids)?;
                for extension_id in &allowed_extension_ids {
                    if !seen_extension_ids.insert(extension_id.clone()) {
                        bail!("extension id {extension_id} is configured by more than one backend");
                    }
                }
                let native_host_name = backend
                    .native_host_name
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("backend {} must configure native_host_name", backend_id)
                    })?;
                WalletBackendConfig::StarmaskExtension(StarmaskExtensionBackendConfig::new(
                    backend_id.to_owned(),
                    instance_label.clone(),
                    approval_surface,
                    allowed_extension_ids,
                    native_host_name,
                    backend.profile_hint.clone(),
                ))
            }
            BackendKind::LocalAccountDir => {
                let prompt_mode = backend.prompt_mode.ok_or_else(|| {
                    anyhow::anyhow!("backend {} must configure prompt_mode", backend_id)
                })?;
                if !matches!(
                    approval_surface,
                    ApprovalSurface::TtyPrompt | ApprovalSurface::DesktopPrompt
                ) {
                    bail!(
                        "backend {} must use tty_prompt or desktop_prompt approval surface",
                        backend_id
                    );
                }
                if prompt_mode.approval_surface() != approval_surface {
                    bail!(
                        "backend {} must use matching approval_surface and prompt_mode",
                        backend_id
                    );
                }
                if prompt_mode != LocalPromptMode::TtyPrompt {
                    bail!(
                        "backend {} must use prompt_mode = tty_prompt until desktop_prompt is implemented",
                        backend_id
                    );
                }
                let account_dir = backend.account_dir.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("backend {} must configure account_dir", backend_id)
                })?;
                let account_dir = validate_local_account_dir(
                    account_dir,
                    backend.require_strict_permissions.unwrap_or(true),
                )?;
                let unlock_cache_ttl = backend.unlock_cache_ttl_seconds.ok_or_else(|| {
                    anyhow::anyhow!(
                        "backend {} must configure unlock_cache_ttl_seconds",
                        backend_id
                    )
                })?;
                if unlock_cache_ttl == 0 {
                    bail!(
                        "backend {} unlock_cache_ttl_seconds must be greater than zero",
                        backend_id
                    );
                }
                let chain_id = backend.chain_id.ok_or_else(|| {
                    anyhow::anyhow!("backend {} must configure chain_id", backend_id)
                })?;
                WalletBackendConfig::LocalAccountDir(LocalAccountDirBackendConfig::new(
                    backend_id.to_owned(),
                    instance_label.clone(),
                    approval_surface,
                    account_dir,
                    prompt_mode,
                    chain_id,
                    DurationSeconds::new(unlock_cache_ttl),
                    backend.allow_read_only_accounts.unwrap_or(true),
                    backend.require_strict_permissions.unwrap_or(true),
                ))
            }
            BackendKind::PrivateKeyDev => unreachable!("validated above"),
        };

        enabled_backends.push(parsed);
    }

    Ok(enabled_backends)
}

fn validate_local_account_dir(path: &Path, require_strict_permissions: bool) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize account_dir {}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("failed to read account_dir {}", canonical.display()))?;
    if !metadata.is_dir() {
        bail!("account_dir {} must be a directory", canonical.display());
    }

    validate_symlink_escapes(&canonical)?;
    if require_strict_permissions {
        validate_directory_permissions(&canonical)?;
    }
    Ok(canonical)
}

fn validate_symlink_escapes(root: &Path) -> Result<()> {
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to scan {}", root.display()))?;
        if entry.file_type().is_symlink() {
            let target = fs::canonicalize(entry.path())
                .with_context(|| format!("failed to resolve symlink {}", entry.path().display()))?;
            if !target.starts_with(root) {
                bail!(
                    "symlink {} escapes the configured account_dir",
                    entry.path().display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_directory_permissions(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read permissions for {}", path.display()))?;
    if metadata.uid() != Uid::effective().as_raw() {
        bail!(
            "account_dir {} must be owned by the current user",
            path.display()
        );
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        bail!(
            "account_dir {} must not grant any group or world permissions",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn read_non_empty_string(field: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{field} cannot be empty");
    }
    Ok(value.to_owned())
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
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};

    use tempfile::tempdir;

    use super::{
        ApprovalSurface, BackendKind, Channel, FileConfig, FileWalletBackendConfig,
        build_phase2_backends, default_native_host_name, read_extension_ids,
        validate_allowed_extension_ids,
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

    #[test]
    fn legacy_extension_settings_translate_to_one_implicit_backend() {
        let config = FileConfig {
            allowed_extension_ids: Some(vec!["ext.allowed".to_owned()]),
            native_host_name: Some("com.starcoin.test".to_owned()),
            ..Default::default()
        };

        let backends =
            super::build_wallet_backends_with_legacy_env(Channel::Development, &config, None, None)
                .unwrap();
        assert_eq!(backends.len(), 1);
        let backend = backends[0].as_extension().unwrap();
        assert_eq!(backend.common.backend_id, "browser-default");
        assert_eq!(backend.common.instance_label, "Browser Default");
        assert!(backend.allowed_extension_ids.contains("ext.allowed"));
        assert_eq!(backend.native_host_name, "com.starcoin.test");
    }

    #[test]
    fn phase2_backend_ids_must_be_unique() {
        let error = build_phase2_backends(
            Channel::Development,
            &[
                FileWalletBackendConfig {
                    backend_id: "dup".to_owned(),
                    backend_kind: BackendKind::StarmaskExtension,
                    enabled: true,
                    instance_label: "one".to_owned(),
                    approval_surface: ApprovalSurface::BrowserUi,
                    allowed_extension_ids: Some(vec!["ext-a".to_owned()]),
                    native_host_name: Some("com.starcoin.test".to_owned()),
                    profile_hint: None,
                    account_dir: None,
                    prompt_mode: None,
                    chain_id: None,
                    unlock_cache_ttl_seconds: None,
                    allow_read_only_accounts: None,
                    require_strict_permissions: None,
                },
                FileWalletBackendConfig {
                    backend_id: "dup".to_owned(),
                    backend_kind: BackendKind::StarmaskExtension,
                    enabled: true,
                    instance_label: "two".to_owned(),
                    approval_surface: ApprovalSurface::BrowserUi,
                    allowed_extension_ids: Some(vec!["ext-b".to_owned()]),
                    native_host_name: Some("com.starcoin.test".to_owned()),
                    profile_hint: None,
                    account_dir: None,
                    prompt_mode: None,
                    chain_id: None,
                    unlock_cache_ttl_seconds: None,
                    allow_read_only_accounts: None,
                    require_strict_permissions: None,
                },
            ],
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate backend_id"));
    }

    #[test]
    fn local_account_dir_requires_matching_prompt_mode() {
        let dir = tempdir().unwrap();
        #[cfg(unix)]
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let error = build_phase2_backends(
            Channel::Development,
            &[FileWalletBackendConfig {
                backend_id: "local-main".to_owned(),
                backend_kind: BackendKind::LocalAccountDir,
                enabled: true,
                instance_label: "Local Main".to_owned(),
                approval_surface: ApprovalSurface::DesktopPrompt,
                allowed_extension_ids: None,
                native_host_name: None,
                profile_hint: None,
                account_dir: Some(dir.path().to_path_buf()),
                prompt_mode: Some(super::LocalPromptMode::TtyPrompt),
                chain_id: Some(251),
                unlock_cache_ttl_seconds: Some(60),
                allow_read_only_accounts: None,
                require_strict_permissions: Some(false),
            }],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("matching approval_surface and prompt_mode")
        );
    }

    #[test]
    fn local_account_dir_rejects_desktop_prompt_until_implemented() {
        let dir = tempdir().unwrap();
        #[cfg(unix)]
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let error = build_phase2_backends(
            Channel::Development,
            &[FileWalletBackendConfig {
                backend_id: "local-main".to_owned(),
                backend_kind: BackendKind::LocalAccountDir,
                enabled: true,
                instance_label: "Local Main".to_owned(),
                approval_surface: ApprovalSurface::DesktopPrompt,
                allowed_extension_ids: None,
                native_host_name: None,
                profile_hint: None,
                account_dir: Some(dir.path().to_path_buf()),
                prompt_mode: Some(super::LocalPromptMode::DesktopPrompt),
                chain_id: Some(251),
                unlock_cache_ttl_seconds: Some(60),
                allow_read_only_accounts: None,
                require_strict_permissions: Some(false),
            }],
        )
        .unwrap_err();

        assert!(error.to_string().contains("prompt_mode = tty_prompt"));
    }

    #[cfg(unix)]
    #[test]
    fn local_account_dir_rejects_group_or_world_accessible_permissions() {
        let dir = tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let error = build_phase2_backends(
            Channel::Development,
            &[FileWalletBackendConfig {
                backend_id: "local-main".to_owned(),
                backend_kind: BackendKind::LocalAccountDir,
                enabled: true,
                instance_label: "Local Main".to_owned(),
                approval_surface: ApprovalSurface::TtyPrompt,
                allowed_extension_ids: None,
                native_host_name: None,
                profile_hint: None,
                account_dir: Some(dir.path().to_path_buf()),
                prompt_mode: Some(super::LocalPromptMode::TtyPrompt),
                chain_id: Some(251),
                unlock_cache_ttl_seconds: Some(60),
                allow_read_only_accounts: None,
                require_strict_permissions: Some(true),
            }],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must not grant any group or world permissions")
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_account_dir_rejects_symlink_escape() {
        let dir = tempdir().unwrap();
        let escaped = tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(escaped.path(), fs::Permissions::from_mode(0o700)).unwrap();
        symlink(escaped.path(), dir.path().join("escape")).unwrap();

        let error = build_phase2_backends(
            Channel::Development,
            &[FileWalletBackendConfig {
                backend_id: "local-main".to_owned(),
                backend_kind: BackendKind::LocalAccountDir,
                enabled: true,
                instance_label: "Local Main".to_owned(),
                approval_surface: ApprovalSurface::TtyPrompt,
                allowed_extension_ids: None,
                native_host_name: None,
                profile_hint: None,
                account_dir: Some(dir.path().to_path_buf()),
                prompt_mode: Some(super::LocalPromptMode::TtyPrompt),
                chain_id: Some(251),
                unlock_cache_ttl_seconds: Some(60),
                allow_read_only_accounts: None,
                require_strict_permissions: Some(false),
            }],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("escapes the configured account_dir")
        );
    }

    #[test]
    fn local_account_dir_rejects_missing_path() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");
        let error = build_phase2_backends(
            Channel::Development,
            &[FileWalletBackendConfig {
                backend_id: "local-main".to_owned(),
                backend_kind: BackendKind::LocalAccountDir,
                enabled: true,
                instance_label: "Local Main".to_owned(),
                approval_surface: ApprovalSurface::TtyPrompt,
                allowed_extension_ids: None,
                native_host_name: None,
                profile_hint: None,
                account_dir: Some(missing.clone()),
                prompt_mode: Some(super::LocalPromptMode::TtyPrompt),
                chain_id: Some(251),
                unlock_cache_ttl_seconds: Some(60),
                allow_read_only_accounts: None,
                require_strict_permissions: Some(false),
            }],
        )
        .unwrap_err();

        assert!(error.to_string().contains(&format!(
            "failed to canonicalize account_dir {}",
            missing.display()
        )));
    }

    #[cfg(unix)]
    #[test]
    fn local_account_dir_requires_chain_id() {
        let dir = tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let error = build_phase2_backends(
            Channel::Development,
            &[FileWalletBackendConfig {
                backend_id: "local-main".to_owned(),
                backend_kind: BackendKind::LocalAccountDir,
                enabled: true,
                instance_label: "Local Main".to_owned(),
                approval_surface: ApprovalSurface::TtyPrompt,
                allowed_extension_ids: None,
                native_host_name: None,
                profile_hint: None,
                account_dir: Some(dir.path().to_path_buf()),
                prompt_mode: Some(super::LocalPromptMode::TtyPrompt),
                chain_id: None,
                unlock_cache_ttl_seconds: Some(60),
                allow_read_only_accounts: None,
                require_strict_permissions: Some(false),
            }],
        )
        .unwrap_err();

        assert!(error.to_string().contains("must configure chain_id"));
    }

    #[test]
    fn legacy_fields_conflict_with_wallet_backends() {
        let config = FileConfig {
            allowed_extension_ids: Some(vec!["ext-a".to_owned()]),
            wallet_backends: Some(vec![FileWalletBackendConfig {
                backend_id: "browser-default".to_owned(),
                backend_kind: BackendKind::StarmaskExtension,
                enabled: true,
                instance_label: "Browser".to_owned(),
                approval_surface: ApprovalSurface::BrowserUi,
                allowed_extension_ids: Some(vec!["ext-a".to_owned()]),
                native_host_name: Some("com.starcoin.test".to_owned()),
                profile_hint: None,
                account_dir: None,
                prompt_mode: None,
                chain_id: None,
                unlock_cache_ttl_seconds: None,
                allow_read_only_accounts: None,
                require_strict_permissions: None,
            }]),
            ..Default::default()
        };

        let error = super::build_wallet_backends(Channel::Development, &config).unwrap_err();
        assert!(error.to_string().contains("legacy extension settings"));
    }
}
