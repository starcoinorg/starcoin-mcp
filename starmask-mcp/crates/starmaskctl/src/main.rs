#![forbid(unsafe_code)]

use std::{
    env, fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use starmask_types::{
    DAEMON_PROTOCOL_VERSION, JsonRpcRequest, JsonRpcResponse, JsonRpcSuccess, SharedError,
    SystemGetInfoParams, SystemGetInfoResult, SystemPingParams, SystemPingResult,
    WalletListAccountsParams, WalletListAccountsResult, WalletStatusParams, WalletStatusResult,
};
use starmaskd::config::{RuntimeConfig, ServeArgs};

#[derive(Debug, Parser)]
#[command(name = "starmaskctl")]
#[command(about = "Diagnostics and maintenance tools for Starmask MCP")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Doctor(DoctorArgs),
}

#[derive(Debug, Args, Clone)]
struct DoctorArgs {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    socket_path: Option<PathBuf>,
    #[arg(long)]
    database_path: Option<PathBuf>,
}

#[derive(Debug)]
struct DatabaseDiagnostics {
    schema_version: u32,
    non_terminal_requests: u64,
    expired_result_payloads: u64,
    connected_wallet_instances: u64,
}

#[derive(Debug)]
struct ManifestDiagnostics {
    path: PathBuf,
    allowed_origins: Vec<String>,
}

#[derive(Default)]
struct DoctorReport {
    failures: usize,
}

impl DoctorReport {
    fn ok(&self, label: &str, detail: impl AsRef<str>) {
        println!("[ok] {label}: {}", detail.as_ref());
    }

    fn fail(&mut self, label: &str, detail: impl AsRef<str>) {
        self.failures += 1;
        println!("[fail] {label}: {}", detail.as_ref());
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Doctor(DoctorArgs {
        config: None,
        socket_path: None,
        database_path: None,
    }));

    match command {
        Command::Doctor(args) => run_doctor(args),
    }
}

fn run_doctor(args: DoctorArgs) -> Result<()> {
    let mut report = DoctorReport::default();
    let config = match RuntimeConfig::load(ServeArgs {
        config: args.config,
        socket_path: args.socket_path,
        database_path: args.database_path,
        log_level: None,
    }) {
        Ok(config) => {
            report.ok(
                "config",
                format!(
                    "channel={:?} native_host_name={} allowlist_entries={} socket={} db={}",
                    config.channel,
                    config.native_host_name,
                    config.allowed_extension_ids.len(),
                    config.socket_path.display(),
                    config.database_path.display(),
                ),
            );
            config
        }
        Err(error) => {
            report.fail("config", error.to_string());
            return bail_if_failures(report);
        }
    };

    match inspect_database(&config.database_path) {
        Ok(database) => report.ok(
            "database",
            format!(
                "schema_version={} non_terminal_requests={} expired_result_payloads={} connected_wallet_instances={}",
                database.schema_version,
                database.non_terminal_requests,
                database.expired_result_payloads,
                database.connected_wallet_instances,
            ),
        ),
        Err(error) => report.fail("database", error.to_string()),
    }

    match inspect_manifest(&config.native_host_name, &config.allowed_extension_ids) {
        Ok(manifest) => report.ok(
            "native-host-manifest",
            format!(
                "{} origins={}",
                manifest.path.display(),
                manifest.allowed_origins.join(","),
            ),
        ),
        Err(error) => report.fail("native-host-manifest", error.to_string()),
    }

    let daemon_client = LocalDaemonClient::new(config.socket_path.clone());
    match daemon_client.system_ping() {
        Ok(ping) => report.ok(
            "daemon",
            format!(
                "reachable protocol_version={} daemon_version={}",
                ping.daemon_protocol_version, ping.daemon_version,
            ),
        ),
        Err(error) => {
            report.fail("daemon", error.to_string());
            return bail_if_failures(report);
        }
    }

    match daemon_client.system_get_info() {
        Ok(info) => report.ok(
            "daemon-info",
            format!(
                "socket_scope={} db_schema_version={} result_retention_seconds={} default_request_ttl_seconds={}",
                info.socket_scope,
                info.db_schema_version,
                info.result_retention_seconds,
                info.default_request_ttl_seconds,
            ),
        ),
        Err(error) => report.fail("daemon-info", error.to_string()),
    }

    match daemon_client.wallet_status() {
        Ok(status) => {
            if status.wallet_online {
                report.ok(
                    "wallet",
                    format!(
                        "online instances={} default_wallet_instance_id={}",
                        status.wallet_instances.len(),
                        status
                            .default_wallet_instance_id
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "<none>".to_owned()),
                    ),
                );
            } else {
                report.fail(
                    "wallet",
                    "no connected wallet instance is currently registered",
                );
            }
        }
        Err(error) => report.fail("wallet", error.to_string()),
    }

    match daemon_client.wallet_list_accounts() {
        Ok(accounts) => {
            let visible_accounts = accounts
                .wallet_instances
                .iter()
                .map(|group| group.accounts.len())
                .sum::<usize>();
            if visible_accounts > 0 {
                report.ok("accounts", format!("visible_accounts={visible_accounts}"));
            } else {
                report.fail("accounts", "no wallet accounts are currently visible");
            }
        }
        Err(error) => report.fail("accounts", error.to_string()),
    }

    bail_if_failures(report)
}

fn bail_if_failures(report: DoctorReport) -> Result<()> {
    if report.failures == 0 {
        Ok(())
    } else {
        bail!("doctor found {} failing checks", report.failures)
    }
}

fn inspect_database(path: &Path) -> Result<DatabaseDiagnostics> {
    if !path.exists() {
        bail!("database file is missing at {}", path.display());
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open database at {}", path.display()))?;
    let schema_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context("failed to read SQLite schema version")?;
    let now_millis = current_time_millis();

    Ok(DatabaseDiagnostics {
        schema_version: u32::try_from(schema_version).context("schema version is negative")?,
        non_terminal_requests: query_count(
            &connection,
            "SELECT COUNT(*) FROM requests WHERE status IN ('created', 'dispatched', 'pending_user_approval')",
            params![],
        )?,
        expired_result_payloads: query_count(
            &connection,
            "SELECT COUNT(*) FROM requests WHERE result_expires_at IS NOT NULL AND result_expires_at <= ?1",
            params![now_millis],
        )?,
        connected_wallet_instances: query_count(
            &connection,
            "SELECT COUNT(*) FROM wallet_instances WHERE connected = 1",
            params![],
        )?,
    })
}

fn query_count<P>(connection: &Connection, sql: &str, params: P) -> Result<u64>
where
    P: rusqlite::Params,
{
    let count: i64 = connection
        .query_row(sql, params, |row| row.get(0))
        .with_context(|| format!("failed to execute count query: {sql}"))?;
    u64::try_from(count).context("count query returned a negative value")
}

fn inspect_manifest(
    native_host_name: &str,
    allowed_extension_ids: &std::collections::BTreeSet<String>,
) -> Result<ManifestDiagnostics> {
    inspect_manifest_in_candidates(
        native_host_name,
        allowed_extension_ids,
        native_host_manifest_candidates(native_host_name),
    )
}

fn inspect_manifest_in_candidates<I>(
    native_host_name: &str,
    allowed_extension_ids: &std::collections::BTreeSet<String>,
    candidates: I,
) -> Result<ManifestDiagnostics>
where
    I: IntoIterator<Item = PathBuf>,
{
    let manifest_path = candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "native host manifest {}.json was not found in the standard Chrome paths",
                native_host_name
            )
        })?;
    let content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let manifest_name = value
        .get("name")
        .and_then(Value::as_str)
        .context("manifest is missing the string field `name`")?;
    if manifest_name != native_host_name {
        bail!(
            "manifest name mismatch: expected {}, found {}",
            native_host_name,
            manifest_name
        );
    }

    let allowed_origins = value
        .get("allowed_origins")
        .and_then(Value::as_array)
        .context("manifest is missing the array field `allowed_origins`")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .context("manifest contains a non-string allowed origin")
        })
        .collect::<Result<Vec<_>>>()?;

    for extension_id in allowed_extension_ids {
        let expected_origin = format!("chrome-extension://{extension_id}/");
        if !allowed_origins
            .iter()
            .any(|origin| origin == &expected_origin)
        {
            bail!("manifest is missing allowed origin {}", expected_origin);
        }
    }

    Ok(ManifestDiagnostics {
        path: manifest_path,
        allowed_origins,
    })
}

fn native_host_manifest_candidates(native_host_name: &str) -> Vec<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if cfg!(target_os = "macos") {
        vec![
            home.join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome")
                .join("NativeMessagingHosts")
                .join(format!("{native_host_name}.json")),
            home.join("Library")
                .join("Application Support")
                .join("Chromium")
                .join("NativeMessagingHosts")
                .join(format!("{native_host_name}.json")),
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            home.join(".config")
                .join("google-chrome")
                .join("NativeMessagingHosts")
                .join(format!("{native_host_name}.json")),
            home.join(".config")
                .join("chromium")
                .join("NativeMessagingHosts")
                .join(format!("{native_host_name}.json")),
        ]
    } else {
        Vec::new()
    }
}

fn current_time_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[derive(Clone, Debug)]
struct LocalDaemonClient {
    socket_path: PathBuf,
}

impl LocalDaemonClient {
    fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    fn system_ping(&self) -> Result<SystemPingResult> {
        self.call(
            "system.ping",
            SystemPingParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
            },
        )
    }

    fn system_get_info(&self) -> Result<SystemGetInfoResult> {
        self.call(
            "system.getInfo",
            SystemGetInfoParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
            },
        )
    }

    fn wallet_status(&self) -> Result<WalletStatusResult> {
        self.call(
            "wallet.status",
            WalletStatusParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
            },
        )
    }

    fn wallet_list_accounts(&self) -> Result<WalletListAccountsResult> {
        self.call(
            "wallet.listAccounts",
            WalletListAccountsParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                wallet_instance_id: None,
                include_public_key: false,
            },
        )
    }

    fn call<P, R>(&self, method: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let request = JsonRpcRequest::new("starmaskctl", method, params);
        let encoded = serde_json::to_vec(&request).context("failed to encode daemon request")?;

        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("failed to connect to {}", self.socket_path.display()))?;
        stream
            .write_all(&encoded)
            .context("failed to send daemon request")?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .context("failed to close daemon request writer")?;

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .context("failed to read daemon response")?;

        let response: JsonRpcResponse<R> =
            serde_json::from_slice(&response).context("failed to decode daemon response")?;
        match response {
            JsonRpcResponse::Success(JsonRpcSuccess { result, .. }) => Ok(result),
            JsonRpcResponse::Error(error) => Err(anyhow::anyhow!(SharedError {
                code: error.error.code,
                message: error.error.message,
                retryable: error.error.retryable,
                details: error.error.details,
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use tempfile::tempdir;

    use super::{inspect_database, inspect_manifest_in_candidates};

    fn extension_ids() -> BTreeSet<String> {
        BTreeSet::from(["ext.allowed".to_owned()])
    }

    #[test]
    fn inspect_database_reports_missing_file() {
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("missing.sqlite3");

        let error = inspect_database(&database_path).unwrap_err();

        assert!(error.to_string().contains("database file is missing"));
    }

    #[test]
    fn inspect_manifest_reports_missing_file() {
        let tempdir = tempdir().unwrap();
        let manifest_path = tempdir.path().join("missing.json");

        let error =
            inspect_manifest_in_candidates("com.starcoin.test", &extension_ids(), [manifest_path])
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("native host manifest com.starcoin.test.json was not found")
        );
    }

    #[test]
    fn inspect_manifest_accepts_matching_allowed_origins() {
        let tempdir = tempdir().unwrap();
        let manifest_path = tempdir.path().join("com.starcoin.test.json");
        fs::write(
            &manifest_path,
            r#"{
                "name": "com.starcoin.test",
                "allowed_origins": ["chrome-extension://ext.allowed/"]
            }"#,
        )
        .unwrap();

        let diagnostics = inspect_manifest_in_candidates(
            "com.starcoin.test",
            &extension_ids(),
            [manifest_path.clone()],
        )
        .unwrap();

        assert_eq!(diagnostics.path, manifest_path);
        assert_eq!(
            diagnostics.allowed_origins,
            vec!["chrome-extension://ext.allowed/".to_owned()]
        );
    }

    #[test]
    fn inspect_manifest_reports_missing_allowed_origin() {
        let tempdir = tempdir().unwrap();
        let manifest_path = tempdir.path().join("com.starcoin.test.json");
        fs::write(
            &manifest_path,
            r#"{
                "name": "com.starcoin.test",
                "allowed_origins": ["chrome-extension://ext.other/"]
            }"#,
        )
        .unwrap();

        let error =
            inspect_manifest_in_candidates("com.starcoin.test", &extension_ids(), [manifest_path])
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("manifest is missing allowed origin chrome-extension://ext.allowed/")
        );
    }
}
