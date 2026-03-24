use starmask_types::{
    ClientRequestId, DurationSeconds, PresentationId, RequestId, RequestResult,
    WalletAccountRecord, WalletInstanceId,
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
pub struct HeartbeatExtensionCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub presented_request_ids: Vec<RequestId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateExtensionAccountsCommand {
    pub wallet_instance_id: WalletInstanceId,
    pub lock_state: starmask_types::LockState,
    pub accounts: Vec<WalletAccountRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkRequestPresentedCommand {
    pub request_id: RequestId,
    pub wallet_instance_id: WalletInstanceId,
    pub delivery_lease_id: starmask_types::DeliveryLeaseId,
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
    HeartbeatExtension(HeartbeatExtensionCommand),
    UpdateExtensionAccounts(UpdateExtensionAccountsCommand),
    PullNextRequest {
        wallet_instance_id: WalletInstanceId,
    },
    MarkRequestPresented(MarkRequestPresentedCommand),
    ResolveRequest(ResolveRequestCommand),
    RejectRequest(RejectRequestCommand),
    TickMaintenance,
}
