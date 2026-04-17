use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    errors::SharedErrorCode,
    ids::{ClientRequestId, RequestId, WalletInstanceId},
    lifecycle::{
        ApprovalSurface, BackendKind, Curve, LockState, MessageFormat, RejectReasonCode,
        RequestKind, RequestStatus, ResultKind, TransportKind, WalletCapability,
    },
    native_bridge::NativeBridgeAccount,
    payload::RequestResult,
    records::{WalletAccountRecord, WalletInstanceRecord},
    time::{DurationSeconds, TimestampMs},
};

pub const DAEMON_PROTOCOL_VERSION: u32 = 1;
pub const GENERIC_BACKEND_PROTOCOL_VERSION: u32 = 2;
pub const STARMASKD_DB_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SystemPingParams {
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SystemPingResult {
    pub ok: bool,
    pub daemon_protocol_version: u32,
    pub daemon_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SystemGetInfoParams {
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SystemGetInfoResult {
    pub daemon_protocol_version: u32,
    pub daemon_version: String,
    pub socket_scope: String,
    pub db_schema_version: u32,
    pub result_retention_seconds: u64,
    pub default_request_ttl_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletStatusParams {
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletStatusResult {
    pub wallet_available: bool,
    pub wallet_online: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_wallet_instance_id: Option<WalletInstanceId>,
    pub wallet_instances: Vec<WalletInstanceSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletListInstancesParams {
    pub protocol_version: u32,
    #[serde(default)]
    pub connected_only: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletListInstancesResult {
    pub wallet_instances: Vec<WalletInstanceSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletListAccountsParams {
    pub protocol_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_instance_id: Option<WalletInstanceId>,
    #[serde(default)]
    pub include_public_key: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletListAccountsResult {
    pub wallet_instances: Vec<WalletAccountGroup>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletGetPublicKeyParams {
    pub protocol_version: u32,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_instance_id: Option<WalletInstanceId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletGetPublicKeyResult {
    pub wallet_instance_id: WalletInstanceId,
    pub address: String,
    pub public_key: String,
    pub curve: Curve,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletSetAccountLabelParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub address: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletSetAccountLabelResult {
    pub wallet_instance_id: WalletInstanceId,
    pub account: WalletAccountSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateAccountParams {
    pub protocol_version: u32,
    pub client_request_id: ClientRequestId,
    pub wallet_instance_id: WalletInstanceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateExportAccountParams {
    pub protocol_version: u32,
    pub client_request_id: ClientRequestId,
    pub account_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_instance_id: Option<WalletInstanceId>,
    pub output_file: String,
    #[serde(default)]
    pub force: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateImportAccountParams {
    pub protocol_version: u32,
    pub client_request_id: ClientRequestId,
    pub wallet_instance_id: WalletInstanceId,
    pub private_key_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateSignTransactionParams {
    pub protocol_version: u32,
    pub client_request_id: ClientRequestId,
    pub account_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_instance_id: Option<WalletInstanceId>,
    pub chain_id: u64,
    pub raw_txn_bcs_hex: String,
    pub tx_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateSignMessageParams {
    pub protocol_version: u32,
    pub client_request_id: ClientRequestId,
    pub account_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_instance_id: Option<WalletInstanceId>,
    pub message: String,
    pub format: MessageFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<DurationSeconds>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateRequestResult {
    pub request_id: RequestId,
    pub client_request_id: ClientRequestId,
    pub kind: RequestKind,
    pub status: RequestStatus,
    pub wallet_instance_id: WalletInstanceId,
    pub created_at: TimestampMs,
    pub expires_at: TimestampMs,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct GetRequestStatusParams {
    pub protocol_version: u32,
    pub request_id: RequestId,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct GetRequestStatusResult {
    pub request_id: RequestId,
    pub client_request_id: ClientRequestId,
    pub kind: RequestKind,
    pub status: RequestStatus,
    pub wallet_instance_id: WalletInstanceId,
    pub created_at: TimestampMs,
    pub expires_at: TimestampMs,
    pub result_kind: ResultKind,
    pub result_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_expires_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<SharedErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<RequestResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestHasAvailableParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestHasAvailableResult {
    pub wallet_instance_id: WalletInstanceId,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CancelRequestParams {
    pub protocol_version: u32,
    pub request_id: RequestId,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CancelRequestResult {
    pub request_id: RequestId,
    pub status: RequestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<SharedErrorCode>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BackendAccount {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    pub is_default: bool,
    pub is_read_only: bool,
    pub is_locked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BackendRegisterParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub backend_kind: BackendKind,
    pub transport_kind: TransportKind,
    pub approval_surface: ApprovalSurface,
    pub instance_label: String,
    pub lock_state: LockState,
    pub capabilities: Vec<WalletCapability>,
    pub backend_metadata: Value,
    pub accounts: Vec<BackendAccount>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BackendRegisteredResult {
    pub wallet_instance_id: WalletInstanceId,
    pub daemon_protocol_version: u32,
    pub accepted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BackendHeartbeatParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presented_request_ids: Vec<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_state: Option<LockState>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BackendUpdateAccountsParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub lock_state: LockState,
    pub capabilities: Vec<WalletCapability>,
    pub accounts: Vec<BackendAccount>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExtensionRegisterParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub extension_id: String,
    pub extension_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_hint: Option<String>,
    pub lock_state: LockState,
    pub accounts_summary: Vec<NativeBridgeAccount>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExtensionRegisteredResult {
    pub wallet_instance_id: WalletInstanceId,
    pub daemon_protocol_version: u32,
    pub accepted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExtensionHeartbeatParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presented_request_ids: Vec<RequestId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExtensionUpdateAccountsParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub lock_state: LockState,
    pub accounts: Vec<NativeBridgeAccount>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct AckResult {
    pub ok: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestPullNextParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PulledRequest {
    pub request_id: RequestId,
    pub client_request_id: ClientRequestId,
    pub kind: RequestKind,
    pub account_address: String,
    pub payload_hash: crate::PayloadHash,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
    pub resume_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_lease_id: Option<crate::DeliveryLeaseId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_id: Option<crate::PresentationId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_expires_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_txn_bcs_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_format: Option<MessageFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_file: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key_file: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestPullNextResult {
    pub wallet_instance_id: WalletInstanceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<PulledRequest>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestPresentedParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub request_id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_lease_id: Option<crate::DeliveryLeaseId>,
    pub presentation_id: crate::PresentationId,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestResolveParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub request_id: RequestId,
    pub presentation_id: crate::PresentationId,
    pub result_kind: ResultKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_txn_bcs_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_account_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_account_public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_account_curve: Option<Curve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_account_is_default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_account_is_locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_account_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_account_output_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_account_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_account_public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_account_curve: Option<Curve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_account_is_default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_account_is_locked: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestRejectParams {
    pub protocol_version: u32,
    pub wallet_instance_id: WalletInstanceId,
    pub request_id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_id: Option<crate::PresentationId>,
    pub reason_code: RejectReasonCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletInstanceSummary {
    pub wallet_instance_id: WalletInstanceId,
    pub extension_connected: bool,
    pub lock_state: LockState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_hint: Option<String>,
    pub last_seen_at: TimestampMs,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletAccountSummary {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    pub is_default: bool,
    pub is_read_only: bool,
    pub is_locked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletAccountGroup {
    pub wallet_instance_id: WalletInstanceId,
    pub instance_label: String,
    pub extension_connected: bool,
    pub lock_state: LockState,
    pub accounts: Vec<WalletAccountSummary>,
}

impl From<&WalletInstanceRecord> for WalletInstanceSummary {
    fn from(value: &WalletInstanceRecord) -> Self {
        Self {
            wallet_instance_id: value.wallet_instance_id.clone(),
            extension_connected: value.connected,
            lock_state: value.lock_state,
            profile_hint: value.profile_hint.clone(),
            last_seen_at: value.last_seen_at,
        }
    }
}

impl From<&WalletAccountRecord> for WalletAccountSummary {
    fn from(value: &WalletAccountRecord) -> Self {
        Self {
            address: value.address.clone(),
            label: value.label.clone(),
            public_key: value.public_key.clone(),
            is_default: value.is_default,
            is_read_only: value.is_read_only,
            is_locked: value.is_locked,
        }
    }
}
