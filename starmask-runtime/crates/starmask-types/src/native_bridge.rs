use serde::{Deserialize, Serialize};

use crate::{
    errors::SharedErrorCode,
    ids::{
        ClientRequestId, DeliveryLeaseId, PayloadHash, PresentationId, RequestId, WalletInstanceId,
    },
    lifecycle::{Curve, LockState, MessageFormat, RejectReasonCode, RequestKind, ResultKind},
    time::TimestampMs,
};

pub const NATIVE_BRIDGE_PROTOCOL_VERSION: u32 = 1;
pub const NATIVE_BRIDGE_MAX_INBOUND_BYTES: u32 = 64 * 1024 * 1024;
pub const NATIVE_BRIDGE_MAX_OUTBOUND_BYTES: u32 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct NativeBridgeAccount {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    pub is_default: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum NativeBridgeRequest {
    #[serde(rename = "extension.register")]
    ExtensionRegister {
        message_id: String,
        protocol_version: u32,
        wallet_instance_id: WalletInstanceId,
        extension_id: String,
        extension_version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        profile_hint: Option<String>,
        lock_state: LockState,
        accounts_summary: Vec<NativeBridgeAccount>,
    },
    #[serde(rename = "extension.heartbeat")]
    ExtensionHeartbeat {
        message_id: String,
        wallet_instance_id: WalletInstanceId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        presented_request_ids: Vec<RequestId>,
    },
    #[serde(rename = "extension.updateAccounts")]
    ExtensionUpdateAccounts {
        message_id: String,
        wallet_instance_id: WalletInstanceId,
        lock_state: LockState,
        accounts: Vec<NativeBridgeAccount>,
    },
    #[serde(rename = "request.pullNext")]
    RequestPullNext {
        message_id: String,
        wallet_instance_id: WalletInstanceId,
    },
    #[serde(rename = "request.presented")]
    RequestPresented {
        message_id: String,
        wallet_instance_id: WalletInstanceId,
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery_lease_id: Option<DeliveryLeaseId>,
        presentation_id: PresentationId,
    },
    #[serde(rename = "request.resolve")]
    RequestResolve {
        message_id: String,
        wallet_instance_id: WalletInstanceId,
        request_id: RequestId,
        presentation_id: PresentationId,
        result_kind: ResultKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        signed_txn_bcs_hex: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        created_account_address: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        created_account_public_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        created_account_curve: Option<Curve>,
        #[serde(skip_serializing_if = "Option::is_none")]
        created_account_is_default: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        created_account_is_locked: Option<bool>,
    },
    #[serde(rename = "request.reject")]
    RequestReject {
        message_id: String,
        wallet_instance_id: WalletInstanceId,
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        presentation_id: Option<PresentationId>,
        reason_code: RejectReasonCode,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason_message: Option<String>,
    },
}

impl NativeBridgeRequest {
    pub fn message_id(&self) -> &str {
        match self {
            Self::ExtensionRegister { message_id, .. }
            | Self::ExtensionHeartbeat { message_id, .. }
            | Self::ExtensionUpdateAccounts { message_id, .. }
            | Self::RequestPullNext { message_id, .. }
            | Self::RequestPresented { message_id, .. }
            | Self::RequestResolve { message_id, .. }
            | Self::RequestReject { message_id, .. } => message_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum NativeBridgeResponse {
    #[serde(rename = "extension.registered")]
    ExtensionRegistered {
        reply_to: String,
        wallet_instance_id: WalletInstanceId,
        daemon_protocol_version: u32,
        accepted: bool,
    },
    #[serde(rename = "extension.ack")]
    ExtensionAck { reply_to: String },
    #[serde(rename = "request.available")]
    RequestAvailable {
        wallet_instance_id: WalletInstanceId,
    },
    #[serde(rename = "request.next")]
    RequestNext {
        reply_to: String,
        request_id: RequestId,
        client_request_id: ClientRequestId,
        kind: RequestKind,
        account_address: String,
        payload_hash: PayloadHash,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_hint: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_context: Option<String>,
        resume_required: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery_lease_id: Option<DeliveryLeaseId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lease_expires_at: Option<TimestampMs>,
        #[serde(skip_serializing_if = "Option::is_none")]
        presentation_id: Option<PresentationId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        presentation_expires_at: Option<TimestampMs>,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw_txn_bcs_hex: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_format: Option<MessageFormat>,
    },
    #[serde(rename = "request.none")]
    RequestNone {
        reply_to: String,
        wallet_instance_id: WalletInstanceId,
    },
    #[serde(rename = "request.cancelled")]
    RequestCancelled {
        wallet_instance_id: WalletInstanceId,
        request_id: RequestId,
    },
    #[serde(rename = "extension.error")]
    ExtensionError {
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        code: SharedErrorCode,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        retryable: Option<bool>,
    },
}
