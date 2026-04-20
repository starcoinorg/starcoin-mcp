#![forbid(unsafe_code)]

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use starcoin_account::{AccountManager, account_storage::AccountStorage};
use starcoin_config::RocksdbConfig;
use starcoin_types::{
    account_address::AccountAddress,
    genesis_config::ChainId,
    transaction::{RawUserTransaction, Script, SignedUserTransaction, TransactionPayload},
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    task::JoinHandle,
    time::sleep,
};
use tracing::{Dispatch, Level};
use tracing_subscriber::fmt::writer::MakeWriter;

use super::{LocalAccountAgent, Snapshot};
use crate::{
    client::{LocalDaemonClient, daemon_protocol_version},
    request_support::RequestRejection,
    tty_prompt::{ApprovalPrompt, PromptApproval},
};
use starmask_core::{CoordinatorCommand, CoordinatorConfig};
use starmask_types::{
    DAEMON_PROTOCOL_VERSION, GENERIC_BACKEND_PROTOCOL_VERSION, JsonRpcRequest, JsonRpcResponse,
    PresentationId, PulledRequest, RequestHasAvailableParams, RequestPresentedParams,
    WalletCapability, WalletInstanceId,
};
use starmaskd::{
    config::{LocalAccountDirBackendConfig, LocalPromptMode, WalletBackendConfig},
    coordinator_runtime::{CoordinatorHandle, spawn_coordinator},
    server::{ServerPolicy, run_unix_server},
    sqlite_store::SqliteStore,
};

struct StubPrompt {
    response: PromptApproval,
}

impl StubPrompt {
    fn approve(password: Option<&str>) -> Arc<Self> {
        Arc::new(Self {
            response: PromptApproval {
                approved: true,
                password: password.map(str::to_owned),
            },
        })
    }
}

impl ApprovalPrompt for StubPrompt {
    fn prompt_for_request(
        &self,
        _request: &PulledRequest,
        _account_info: &starcoin_account_api::AccountInfo,
        _capabilities: &[WalletCapability],
    ) -> std::result::Result<PromptApproval, RequestRejection> {
        Ok(self.response.clone())
    }

    fn prompt_for_create_account(
        &self,
        _request: &PulledRequest,
        _capabilities: &[WalletCapability],
    ) -> std::result::Result<PromptApproval, RequestRejection> {
        Ok(self.response.clone())
    }

    fn prompt_for_import_account(
        &self,
        _request: &PulledRequest,
        _capabilities: &[WalletCapability],
    ) -> std::result::Result<PromptApproval, RequestRejection> {
        Ok(self.response.clone())
    }
}

struct RealStackHarness {
    _tempdir: TempDir,
    socket_path: PathBuf,
    coordinator: CoordinatorHandle,
    server: JoinHandle<()>,
    agent: Option<LocalAccountAgent>,
    config: LocalAccountDirBackendConfig,
    account_address: AccountAddress,
}

#[derive(Clone, Default)]
struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

struct SharedLogWriter(SharedLogBuffer);

struct TestLogCapture {
    buffer: SharedLogBuffer,
    dispatch: Dispatch,
}

impl SharedLogBuffer {
    fn snapshot(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl TestLogCapture {
    fn snapshot(&self) -> String {
        self.buffer.snapshot()
    }
}

impl<'a> MakeWriter<'a> for SharedLogBuffer {
    type Writer = SharedLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedLogWriter(self.clone())
    }
}

impl io::Write for SharedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn test_log_capture() -> TestLogCapture {
    let buffer = SharedLogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(Level::DEBUG)
        .with_target(false)
        .with_writer(buffer.clone())
        .finish();
    TestLogCapture {
        buffer,
        dispatch: Dispatch::new(subscriber),
    }
}

// LocalDaemonClient uses blocking Unix sockets, so drive agent-side RPC on the
// blocking pool instead of stalling the async daemon runtime.
async fn run_agent_action_on_blocking_thread<R, F>(
    mut agent: LocalAccountAgent,
    dispatch: Option<Dispatch>,
    action: F,
) -> (LocalAccountAgent, R)
where
    R: Send + 'static,
    F: FnOnce(&mut LocalAccountAgent) -> R + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let _guard = dispatch.as_ref().map(tracing::dispatcher::set_default);
        let result = action(&mut agent);
        (agent, result)
    })
    .await
    .unwrap()
}

impl RealStackHarness {
    async fn new(locked: bool, delivery_lease_secs: u64) -> Self {
        let tempdir = TempDir::new().unwrap();
        let socket_path = tempdir.path().join("starmaskd.sock");
        let database_path = tempdir.path().join("starmaskd.sqlite3");
        let account_dir = tempdir.path().join("account");
        std::fs::create_dir_all(&account_dir).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&account_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

        let storage =
            AccountStorage::create_from_path(&account_dir, RocksdbConfig::default()).unwrap();
        let manager = AccountManager::new(storage, ChainId::test()).unwrap();
        let account = manager.create_account("hello").unwrap();
        if locked {
            manager.lock_account(*account.address()).unwrap();
        } else {
            manager
                .unlock_account(*account.address(), "hello", Duration::from_secs(60))
                .unwrap();
        }

        let backend_config = test_config(account_dir.clone());

        let mut coordinator_config = CoordinatorConfig::default();
        coordinator_config.delivery_lease_ttl =
            starmask_types::DurationSeconds::new(delivery_lease_secs);
        let store = SqliteStore::open(&database_path).unwrap();
        let coordinator = spawn_coordinator(store, coordinator_config);
        let server_handle = coordinator.clone();
        let socket_for_server = socket_path.clone();
        let server = tokio::spawn(async move {
            run_unix_server(
                &socket_for_server,
                server_handle,
                ServerPolicy::new(
                    starmask_types::Channel::Development,
                    vec![WalletBackendConfig::LocalAccountDir(backend_config.clone())],
                ),
            )
            .await
            .unwrap();
        });
        wait_for_socket(&socket_path).await;

        let config = test_config(account_dir);
        let agent = LocalAccountAgent::from_parts(
            Arc::new(LocalDaemonClient::new(socket_path.clone())),
            StubPrompt::approve(if locked { Some("hello") } else { None }),
            manager,
            config.clone(),
            Duration::from_secs(1),
        )
        .unwrap();

        Self {
            _tempdir: tempdir,
            socket_path,
            coordinator,
            server,
            agent: Some(agent),
            config,
            account_address: *account.address(),
        }
    }

    fn agent_mut(&mut self) -> &mut LocalAccountAgent {
        self.agent.as_mut().expect("agent should still be running")
    }

    async fn run_agent_action<R, F>(mut self, dispatch: Option<Dispatch>, action: F) -> (Self, R)
    where
        R: Send + 'static,
        F: FnOnce(&mut LocalAccountAgent) -> R + Send + 'static,
    {
        let agent = self.agent.take().expect("agent should still be running");
        let (agent, result) = run_agent_action_on_blocking_thread(agent, dispatch, action).await;
        self.agent = Some(agent);
        (self, result)
    }

    async fn register_backend(self) -> (Self, Snapshot) {
        self.run_agent_action(None, |agent| agent.register_backend().unwrap())
            .await
    }

    async fn register_backend_with_dispatch(self, dispatch: Dispatch) -> (Self, Snapshot) {
        self.run_agent_action(Some(dispatch), |agent| agent.register_backend().unwrap())
            .await
    }

    fn spawn_restarted_agent(&mut self) -> LocalAccountAgent {
        drop(self.agent.take());
        let storage =
            AccountStorage::create_from_path(self.config.account_dir(), RocksdbConfig::default())
                .unwrap();
        let manager = AccountManager::new(storage, ChainId::new(self.config.chain_id())).unwrap();
        LocalAccountAgent::from_parts(
            Arc::new(LocalDaemonClient::new(self.socket_path.clone())),
            StubPrompt::approve(None),
            manager,
            self.config.clone(),
            Duration::from_secs(1),
        )
        .unwrap()
    }

    async fn handle_next_request(self, snapshot: Snapshot) -> Self {
        self.handle_next_request_inner(snapshot, None).await
    }

    async fn handle_next_request_with_dispatch(
        self,
        snapshot: Snapshot,
        dispatch: Dispatch,
    ) -> Self {
        self.handle_next_request_inner(snapshot, Some(dispatch))
            .await
    }

    async fn handle_next_request_inner(
        self,
        snapshot: Snapshot,
        dispatch: Option<Dispatch>,
    ) -> Self {
        let (harness, ()) = self
            .run_agent_action(dispatch, move |agent| {
                let pulled = agent.pull_next_request().unwrap().unwrap();
                agent.handle_request(pulled, &snapshot).unwrap();
            })
            .await;
        harness
    }

    fn raw_sign_transaction_hex(&self) -> String {
        let raw_txn = RawUserTransaction::new_with_default_gas_token(
            self.account_address,
            7,
            TransactionPayload::Script(Script::new(vec![], vec![], vec![])),
            1_000,
            1,
            100_000,
            ChainId::test(),
        );
        format!("0x{}", hex::encode(bcs_ext::to_bytes(&raw_txn).unwrap()))
    }

    fn exported_private_key_hex(&self, password: &str) -> String {
        let private_key = self
            .agent
            .as_ref()
            .unwrap()
            .manager
            .export_account(self.account_address, password)
            .unwrap();
        format!("0x{}", hex::encode(private_key))
    }

    async fn shutdown(self) {
        self.server.abort();
        let _ = self.server.await;
    }
}

fn test_config(account_dir: PathBuf) -> LocalAccountDirBackendConfig {
    LocalAccountDirBackendConfig::new(
        "local-default",
        "Local Default Wallet",
        starmask_types::ApprovalSurface::TtyPrompt,
        account_dir,
        LocalPromptMode::TtyPrompt,
        ChainId::test().id(),
        starmask_types::DurationSeconds::new(30),
        true,
        false,
    )
}

async fn wait_for_socket(socket_path: &Path) {
    for _ in 0..100 {
        if socket_path.exists() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("socket did not appear at {}", socket_path.display());
}

async fn call_daemon(
    socket_path: &Path,
    id: &str,
    method: &str,
    params: Value,
) -> JsonRpcResponse<Value> {
    let mut stream = UnixStream::connect(socket_path).await.unwrap();
    let encoded = serde_json::to_vec(&JsonRpcRequest::new(id, method, params)).unwrap();
    stream.write_all(&encoded).await.unwrap();
    stream.shutdown().await.unwrap();

    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_socket_stack_round_trips_sign_message_and_has_available() {
    let harness = RealStackHarness::new(false, 30).await;
    let (mut harness, snapshot) = harness.register_backend().await;

    let created = call_daemon(
        &harness.socket_path,
        "req-create-message",
        "request.createSignMessage",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "client_request_id": "client-stack-message",
            "account_address": harness.account_address.to_string(),
            "wallet_instance_id": "local-default",
            "message": "hello",
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    let request_id = created.result["request_id"].as_str().unwrap();

    let has_available = call_daemon(
        &harness.socket_path,
        "req-has-available-before",
        "request.hasAvailable",
        json!(RequestHasAvailableParams {
            protocol_version: GENERIC_BACKEND_PROTOCOL_VERSION,
            wallet_instance_id: WalletInstanceId::new("local-default").unwrap(),
        }),
    )
    .await;
    let JsonRpcResponse::Success(has_available) = has_available else {
        panic!("expected hasAvailable success");
    };
    assert_eq!(has_available.result["available"], json!(true));

    harness = harness.handle_next_request(snapshot).await;

    let status = call_daemon(
        &harness.socket_path,
        "req-status-message",
        "request.getStatus",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "request_id": request_id,
        }),
    )
    .await;
    let JsonRpcResponse::Success(status) = status else {
        panic!("expected status success");
    };
    assert_eq!(status.result["status"], json!("approved"));
    assert_eq!(status.result["result"]["kind"], json!("signed_message"));
    assert!(status.result["result"]["signature"].as_str().is_some());

    let has_available = call_daemon(
        &harness.socket_path,
        "req-has-available-after",
        "request.hasAvailable",
        json!(RequestHasAvailableParams {
            protocol_version: GENERIC_BACKEND_PROTOCOL_VERSION,
            wallet_instance_id: WalletInstanceId::new("local-default").unwrap(),
        }),
    )
    .await;
    let JsonRpcResponse::Success(has_available) = has_available else {
        panic!("expected hasAvailable success");
    };
    assert_eq!(has_available.result["available"], json!(false));

    harness.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_socket_stack_round_trips_sign_transaction() {
    let harness = RealStackHarness::new(false, 30).await;
    let (mut harness, snapshot) = harness.register_backend().await;

    let created = call_daemon(
        &harness.socket_path,
        "req-create-transaction",
        "request.createSignTransaction",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "client_request_id": "client-stack-transaction",
            "account_address": harness.account_address.to_string(),
            "wallet_instance_id": "local-default",
            "chain_id": 255,
            "raw_txn_bcs_hex": harness.raw_sign_transaction_hex(),
            "tx_kind": "transfer",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    let request_id = created.result["request_id"].as_str().unwrap();

    harness = harness.handle_next_request(snapshot).await;

    let status = call_daemon(
        &harness.socket_path,
        "req-status-transaction",
        "request.getStatus",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "request_id": request_id,
        }),
    )
    .await;
    let JsonRpcResponse::Success(status) = status else {
        panic!("expected status success");
    };
    assert_eq!(status.result["status"], json!("approved"));
    assert_eq!(status.result["result"]["kind"], json!("signed_transaction"));
    let signed_txn_bcs_hex = status.result["result"]["signed_txn_bcs_hex"]
        .as_str()
        .unwrap();
    let signed_txn_bytes = hex::decode(signed_txn_bcs_hex.trim_start_matches("0x")).unwrap();
    let signed_txn: SignedUserTransaction = bcs_ext::from_bytes(&signed_txn_bytes).unwrap();
    assert_eq!(signed_txn.sender(), harness.account_address);

    harness.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn local_socket_stack_logs_omit_sensitive_signing_material() {
    let captured_logs = test_log_capture();
    let _guard = tracing::dispatcher::set_default(&captured_logs.dispatch);

    let harness = RealStackHarness::new(true, 30).await;
    let (mut harness, snapshot) = harness
        .register_backend_with_dispatch(captured_logs.dispatch.clone())
        .await;
    let password = "hello";
    let secret_message = "message-do-not-log";
    let raw_txn_bcs_hex = harness.raw_sign_transaction_hex();
    let private_key_hex = harness.exported_private_key_hex(password);

    let created = call_daemon(
        &harness.socket_path,
        "req-log-message",
        "request.createSignMessage",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "client_request_id": "client-log-message",
            "account_address": harness.account_address.to_string(),
            "wallet_instance_id": "local-default",
            "message": secret_message,
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(_) = created else {
        panic!("expected create success");
    };

    harness = harness
        .handle_next_request_with_dispatch(snapshot.clone(), captured_logs.dispatch.clone())
        .await;

    let created = call_daemon(
        &harness.socket_path,
        "req-log-transaction",
        "request.createSignTransaction",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "client_request_id": "client-log-transaction",
            "account_address": harness.account_address.to_string(),
            "wallet_instance_id": "local-default",
            "chain_id": 255,
            "raw_txn_bcs_hex": raw_txn_bcs_hex,
            "tx_kind": "transfer",
        }),
    )
    .await;
    let JsonRpcResponse::Success(_) = created else {
        panic!("expected create success");
    };

    harness = harness
        .handle_next_request_with_dispatch(snapshot, captured_logs.dispatch.clone())
        .await;

    let logs = captured_logs.snapshot();
    assert!(!logs.contains(password));
    assert!(!logs.contains(&private_key_hex));
    assert!(!logs.contains(secret_message));
    assert!(!logs.contains(&raw_txn_bcs_hex));

    harness.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_backend_restart_before_presented_requeues_after_lease_expiry() {
    let harness = RealStackHarness::new(false, 1).await;
    let (mut harness, _) = harness.register_backend().await;

    let created = call_daemon(
        &harness.socket_path,
        "req-create-requeue",
        "request.createSignMessage",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "client_request_id": "client-requeue",
            "account_address": harness.account_address.to_string(),
            "wallet_instance_id": "local-default",
            "message": "hello",
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    let request_id = created.result["request_id"].as_str().unwrap().to_owned();

    let first_pull = harness.agent_mut().pull_next_request().unwrap().unwrap();
    assert_eq!(first_pull.request_id.as_str(), request_id);

    let mut restarted_agent = harness.spawn_restarted_agent();
    (restarted_agent, _) = run_agent_action_on_blocking_thread(restarted_agent, None, |agent| {
        agent.register_backend().unwrap()
    })
    .await;
    let (restarted_agent_after_pull, second_pull) =
        run_agent_action_on_blocking_thread(restarted_agent, None, |agent| {
            agent.pull_next_request().unwrap()
        })
        .await;
    let restarted_agent = restarted_agent_after_pull;
    assert!(second_pull.is_none());

    sleep(Duration::from_secs(2)).await;
    harness
        .coordinator
        .dispatch(CoordinatorCommand::TickMaintenance)
        .await
        .unwrap();

    let (_restarted_agent, requeued) =
        run_agent_action_on_blocking_thread(restarted_agent, None, |agent| {
            agent.pull_next_request().unwrap().unwrap()
        })
        .await;
    assert_eq!(requeued.request_id.as_str(), request_id);
    assert!(!requeued.resume_required);
    assert!(requeued.delivery_lease_id.is_some());

    harness.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_backend_restart_after_presented_resumes_same_instance() {
    let harness = RealStackHarness::new(false, 30).await;
    let (mut harness, _) = harness.register_backend().await;

    let created = call_daemon(
        &harness.socket_path,
        "req-create-resume",
        "request.createSignMessage",
        json!({
            "protocol_version": DAEMON_PROTOCOL_VERSION,
            "client_request_id": "client-resume",
            "account_address": harness.account_address.to_string(),
            "wallet_instance_id": "local-default",
            "message": "hello",
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    let request_id = created.result["request_id"].as_str().unwrap().to_owned();

    let first_pull = harness.agent_mut().pull_next_request().unwrap().unwrap();
    let presentation_id = PresentationId::new("presentation-restart").unwrap();
    let wallet_instance_id = harness.agent_mut().wallet_instance_id.clone();
    harness
        .agent_mut()
        .client
        .request_presented(RequestPresentedParams {
            protocol_version: daemon_protocol_version(),
            wallet_instance_id,
            request_id: first_pull.request_id.clone(),
            delivery_lease_id: first_pull.delivery_lease_id.clone(),
            presentation_id: presentation_id.clone(),
        })
        .unwrap();

    let mut restarted_agent = harness.spawn_restarted_agent();
    (restarted_agent, _) = run_agent_action_on_blocking_thread(restarted_agent, None, |agent| {
        agent.register_backend().unwrap()
    })
    .await;
    let (_restarted_agent, resumed) =
        run_agent_action_on_blocking_thread(restarted_agent, None, |agent| {
            agent.pull_next_request().unwrap().unwrap()
        })
        .await;
    assert_eq!(resumed.request_id.as_str(), request_id);
    assert!(resumed.resume_required);
    assert_eq!(resumed.presentation_id, Some(presentation_id));
    assert_eq!(resumed.delivery_lease_id, None);

    harness.shutdown().await;
}
