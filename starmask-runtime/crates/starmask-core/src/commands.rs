use starmask_types::{
    ApprovalSurface, BackendKind, ClientRequestId, DurationSeconds, LockState, PresentationId,
    RequestId, RequestResult, TransportKind, WalletAccountRecord, WalletCapability,
    WalletInstanceId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSignTransactionCommand {
    pub client_request_id: ClientRequestId,
    pub account_address: String,
    pub wallet_instance_id: Option<WalletInstanceId>,
    pub chain_id: u64,
    pub raw_txn_bcs_hex: String,
    pub tx_kind: String,
    pub display_hint: Option<String>,
    pub client_context: Option<String>,
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSignMessageCommand {
    pub client_request_id: ClientRequestId,
    pub account_address: String,
    pub wallet_instance_id: Option<WalletInstanceId>,
    pub message: String,
    pub format: starmask_types::MessageFormat,
    pub display_hint: Option<String>,
    pub client_context: Option<String>,
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateAccountCommand {
    pub client_request_id: ClientRequestId,
    pub wallet_instance_id: WalletInstanceId,
    pub display_hint: Option<String>,
    pub client_context: Option<String>,
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterExtensionCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub extension_id: String,
    pub extension_version: String,
    pub protocol_version: u32,
    pub profile_hint: Option<String>,
    pub lock_state: starmask_types::LockState,
    pub accounts: Vec<WalletAccountRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterBackendCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub backend_kind: BackendKind,
    pub transport_kind: TransportKind,
    pub approval_surface: ApprovalSurface,
    pub instance_label: String,
    pub extension_id: String,
    pub extension_version: String,
    pub protocol_version: u32,
    pub capabilities: Vec<WalletCapability>,
    pub backend_metadata: serde_json::Value,
    pub profile_hint: Option<String>,
    pub lock_state: LockState,
    pub accounts: Vec<WalletAccountRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeartbeatExtensionCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub presented_request_ids: Vec<RequestId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeartbeatBackendCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub presented_request_ids: Vec<RequestId>,
    pub lock_state: Option<LockState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateExtensionAccountsCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub lock_state: starmask_types::LockState,
    pub accounts: Vec<WalletAccountRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateBackendAccountsCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub lock_state: LockState,
    pub capabilities: Vec<WalletCapability>,
    pub accounts: Vec<WalletAccountRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkRequestPresentedCommand {
    pub request_id: RequestId,
    pub wallet_instance_id: WalletInstanceId,
    pub delivery_lease_id: Option<starmask_types::DeliveryLeaseId>,
    pub presentation_id: PresentationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveRequestCommand {
    pub request_id: RequestId,
    pub wallet_instance_id: WalletInstanceId,
    pub presentation_id: PresentationId,
    pub result: RequestResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectRequestCommand {
    pub request_id: RequestId,
    pub wallet_instance_id: WalletInstanceId,
    pub presentation_id: Option<PresentationId>,
    pub reason_code: starmask_types::RejectReasonCode,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoordinatorCommand {
    SystemPing,
    SystemGetInfo,
    WalletStatus,
    WalletListInstances {
        connected_only: bool,
    },
    WalletListAccounts {
        wallet_instance_id: Option<WalletInstanceId>,
        include_public_key: bool,
    },
    WalletGetPublicKey {
        address: String,
        wallet_instance_id: Option<WalletInstanceId>,
    },
    CreateAccount(CreateAccountCommand),
    CreateSignTransaction(CreateSignTransactionCommand),
    CreateSignMessage(CreateSignMessageCommand),
    GetRequestStatus {
        request_id: RequestId,
    },
    RequestHasAvailable {
        wallet_instance_id: WalletInstanceId,
    },
    CancelRequest {
        request_id: RequestId,
    },
    RegisterExtension(RegisterExtensionCommand),
    RegisterBackend(RegisterBackendCommand),
    HeartbeatExtension(HeartbeatExtensionCommand),
    HeartbeatBackend(HeartbeatBackendCommand),
    UpdateExtensionAccounts(UpdateExtensionAccountsCommand),
    UpdateBackendAccounts(UpdateBackendAccountsCommand),
    PullNextRequest {
        wallet_instance_id: WalletInstanceId,
    },
    MarkRequestPresented(MarkRequestPresentedCommand),
    ResolveRequest(ResolveRequestCommand),
    RejectRequest(RejectRequestCommand),
    TickMaintenance,
}
