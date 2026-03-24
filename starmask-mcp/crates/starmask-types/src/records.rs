use serde::{Deserialize, Serialize};

use crate::{
    errors::SharedErrorCode,
    ids::{
        ClientRequestId, DeliveryLeaseId, PayloadHash, PresentationId, RequestId, WalletInstanceId,
    },
    lifecycle::{LockState, RejectReasonCode, RequestKind, RequestStatus},
    payload::{RequestPayload, RequestResult},
    time::TimestampMs,
};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct DeliveryLease {
    pub delivery_lease_id: DeliveryLeaseId,
    pub delivery_lease_expires_at: TimestampMs,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PresentationLease {
    pub presentation_id: PresentationId,
    pub presentation_expires_at: TimestampMs,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct RequestRecord {
    pub request_id: RequestId,
    pub client_request_id: ClientRequestId,
    pub kind: RequestKind,
    pub status: RequestStatus,
    pub wallet_instance_id: WalletInstanceId,
    pub account_address: String,
    pub payload_hash: PayloadHash,
    pub payload: RequestPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<RequestResult>,
    pub created_at: TimestampMs,
    pub expires_at: TimestampMs,
    pub updated_at: TimestampMs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_expires_at: Option<TimestampMs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<SharedErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_reason_code: Option<RejectReasonCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_lease: Option<DeliveryLease>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<PresentationLease>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletInstanceRecord {
    pub wallet_instance_id: WalletInstanceId,
    pub extension_id: String,
    pub extension_version: String,
    pub protocol_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_hint: Option<String>,
    pub lock_state: LockState,
    pub connected: bool,
    pub last_seen_at: TimestampMs,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct WalletAccountRecord {
    pub wallet_instance_id: WalletInstanceId,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    pub is_default: bool,
    pub is_locked: bool,
    pub last_seen_at: TimestampMs,
}
