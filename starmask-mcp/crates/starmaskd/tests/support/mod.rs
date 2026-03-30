#![allow(dead_code)]

use std::path::Path;

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::{Duration, sleep},
};

use starmask_core::{
    AllowAllPolicy, Clock, Coordinator, CoordinatorCommand, CoordinatorConfig, CoordinatorResponse,
    CoreResult, IdGenerator,
    commands::{
        CreateSignTransactionCommand, MarkRequestPresentedCommand, RegisterBackendCommand,
        RegisterExtensionCommand, ResolveRequestCommand, UpdateExtensionAccountsCommand,
    },
};
use starmask_types::{
    ApprovalSurface, BackendKind, ClientRequestId, DeliveryLeaseId, JsonRpcRequest,
    JsonRpcResponse, LockState, NativeBridgeAccount, PresentationId, RequestId, RequestResult,
    TimestampMs, TransportKind, WalletAccountRecord, WalletCapability, WalletInstanceId,
};
use starmaskd::sqlite_store::SqliteStore;

pub const BASE_TIME_MS: i64 = 1_710_000_000_000;

#[derive(Clone, Copy)]
pub struct FixedClock {
    pub now: TimestampMs,
}

impl Clock for FixedClock {
    fn now(&self) -> TimestampMs {
        self.now
    }
}

#[derive(Default)]
pub struct SequentialIds {
    next: u64,
}

impl IdGenerator for SequentialIds {
    fn new_request_id(&mut self) -> CoreResult<RequestId> {
        self.next += 1;
        RequestId::new(format!("req-{}", self.next))
            .map_err(|error| starmask_core::CoreError::Invariant(error.to_string()))
    }

    fn new_delivery_lease_id(&mut self) -> CoreResult<DeliveryLeaseId> {
        self.next += 1;
        DeliveryLeaseId::new(format!("lease-{}", self.next))
            .map_err(|error| starmask_core::CoreError::Invariant(error.to_string()))
    }
}

pub fn open_coordinator(
    database_path: &Path,
    now: TimestampMs,
) -> Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds> {
    open_coordinator_with_config(database_path, now, CoordinatorConfig::default())
}

pub fn open_coordinator_with_config(
    database_path: &Path,
    now: TimestampMs,
    config: CoordinatorConfig,
) -> Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds> {
    Coordinator::new(
        SqliteStore::open(database_path).expect("sqlite store should open"),
        AllowAllPolicy,
        FixedClock { now },
        SequentialIds::default(),
        config,
    )
}

pub fn wallet_account(
    wallet_instance_id: &WalletInstanceId,
    address: &str,
    is_default: bool,
) -> WalletAccountRecord {
    WalletAccountRecord {
        wallet_instance_id: wallet_instance_id.clone(),
        address: address.to_owned(),
        label: None,
        public_key: None,
        is_default,
        is_read_only: false,
        is_locked: false,
        last_seen_at: TimestampMs::from_millis(0),
    }
}

pub fn native_bridge_account(address: &str, is_default: bool) -> NativeBridgeAccount {
    NativeBridgeAccount {
        address: address.to_owned(),
        label: None,
        public_key: None,
        is_default,
    }
}

pub fn register_wallet(
    coordinator: &mut Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds>,
    wallet_instance_id: &WalletInstanceId,
    lock_state: LockState,
    accounts: Vec<WalletAccountRecord>,
) {
    coordinator
        .dispatch(CoordinatorCommand::RegisterExtension(
            RegisterExtensionCommand {
                wallet_instance_id: wallet_instance_id.clone(),
                extension_id: "ext.allowed".to_owned(),
                extension_version: "1.0.0".to_owned(),
                protocol_version: 1,
                profile_hint: None,
                lock_state,
                accounts: Vec::new(),
            },
        ))
        .expect("wallet registration should succeed");
    coordinator
        .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
            UpdateExtensionAccountsCommand {
                wallet_instance_id: wallet_instance_id.clone(),
                lock_state,
                accounts,
            },
        ))
        .expect("wallet account update should succeed");
}

pub fn register_local_backend(
    coordinator: &mut Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds>,
    wallet_instance_id: &WalletInstanceId,
    lock_state: LockState,
    accounts: Vec<WalletAccountRecord>,
) {
    coordinator
        .dispatch(CoordinatorCommand::RegisterBackend(
            RegisterBackendCommand {
                wallet_instance_id: wallet_instance_id.clone(),
                backend_kind: BackendKind::LocalAccountDir,
                transport_kind: TransportKind::LocalSocket,
                approval_surface: ApprovalSurface::TtyPrompt,
                instance_label: "Local Main".to_owned(),
                extension_id: String::new(),
                extension_version: String::new(),
                protocol_version: 2,
                capabilities: vec![
                    WalletCapability::Unlock,
                    WalletCapability::GetPublicKey,
                    WalletCapability::SignMessage,
                    WalletCapability::SignTransaction,
                ],
                backend_metadata: serde_json::json!({
                    "account_provider_kind": "local",
                    "prompt_mode": "tty_prompt",
                }),
                profile_hint: None,
                lock_state,
                accounts,
            },
        ))
        .expect("local backend registration should succeed");
}

pub fn create_sign_transaction(
    coordinator: &mut Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds>,
    client_request_id: &str,
    wallet_instance_id: &WalletInstanceId,
) -> starmask_types::CreateRequestResult {
    let created = coordinator
        .dispatch(CoordinatorCommand::CreateSignTransaction(
            CreateSignTransactionCommand {
                client_request_id: ClientRequestId::new(client_request_id).unwrap(),
                account_address: "0x1".to_owned(),
                wallet_instance_id: Some(wallet_instance_id.clone()),
                chain_id: 251,
                raw_txn_bcs_hex: "0xabc".to_owned(),
                tx_kind: "transfer".to_owned(),
                display_hint: None,
                client_context: None,
                ttl_seconds: None,
            },
        ))
        .expect("request creation should succeed");
    let CoordinatorResponse::RequestCreated(created) = created else {
        panic!("unexpected response");
    };
    created
}

pub fn pull_next_request(
    coordinator: &mut Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds>,
    wallet_instance_id: &WalletInstanceId,
) -> starmask_types::PulledRequest {
    let pulled = coordinator
        .dispatch(CoordinatorCommand::PullNextRequest {
            wallet_instance_id: wallet_instance_id.clone(),
        })
        .expect("pull next should succeed");
    let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
        panic!("unexpected response");
    };
    pulled.request.expect("request should be available")
}

pub fn mark_presented(
    coordinator: &mut Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds>,
    request_id: &RequestId,
    wallet_instance_id: &WalletInstanceId,
    delivery_lease_id: DeliveryLeaseId,
    presentation_id: PresentationId,
) {
    coordinator
        .dispatch(CoordinatorCommand::MarkRequestPresented(
            MarkRequestPresentedCommand {
                request_id: request_id.clone(),
                wallet_instance_id: wallet_instance_id.clone(),
                delivery_lease_id: Some(delivery_lease_id),
                presentation_id,
            },
        ))
        .expect("mark presented should succeed");
}

pub fn resolve_transaction_request(
    coordinator: &mut Coordinator<SqliteStore, AllowAllPolicy, FixedClock, SequentialIds>,
    request_id: &RequestId,
    wallet_instance_id: &WalletInstanceId,
    presentation_id: PresentationId,
) {
    coordinator
        .dispatch(CoordinatorCommand::ResolveRequest(ResolveRequestCommand {
            request_id: request_id.clone(),
            wallet_instance_id: wallet_instance_id.clone(),
            presentation_id,
            result: RequestResult::SignedTransaction {
                signed_txn_bcs_hex: "0xsigned".to_owned(),
            },
        }))
        .expect("resolve request should succeed");
}

pub async fn wait_for_socket(socket_path: &Path) {
    for _ in 0..100 {
        if socket_path.exists() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("socket did not appear at {}", socket_path.display());
}

pub async fn call_daemon(
    socket_path: &Path,
    id: &str,
    method: &str,
    params: Value,
) -> JsonRpcResponse<Value> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .expect("daemon socket should accept connections");
    let encoded = serde_json::to_vec(&JsonRpcRequest::new(id, method, params))
        .expect("request should encode");
    stream
        .write_all(&encoded)
        .await
        .expect("request write should succeed");
    stream
        .shutdown()
        .await
        .expect("request shutdown should succeed");

    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .await
        .expect("response read should succeed");
    serde_json::from_slice(&bytes).expect("response should decode")
}
