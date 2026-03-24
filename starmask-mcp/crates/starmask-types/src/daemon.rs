use serde::{Deserialize, Serialize};

use crate::{
    errors::SharedErrorCode,
    ids::{ClientRequestId, RequestId, WalletInstanceId},
    lifecycle::{Curve, LockState, MessageFormat, RequestKind, RequestStatus, ResultKind},
    payload::RequestResult,
    records::{WalletAccountRecord, WalletInstanceRecord},
    time::{DurationSeconds, TimestampMs},
};

pub const DAEMON_PROTOCOL_VERSION: u32 = 1;

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
    pub is_locked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletAccountGroup {
    pub wallet_instance_id: WalletInstanceId,
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
            is_locked: value.is_locked,
        }
    }
}
