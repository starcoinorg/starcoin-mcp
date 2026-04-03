#![forbid(unsafe_code)]

pub mod daemon;
pub mod errors;
pub mod ids;
pub mod jsonrpc;
pub mod lifecycle;
pub mod native_bridge;
pub mod payload;
pub mod records;
pub mod time;

pub use daemon::*;
pub use errors::{IdValidationError, SharedError, SharedErrorCode};
pub use ids::{
    ClientRequestId, DeliveryLeaseId, PayloadHash, PresentationId, RequestId, WalletInstanceId,
};
pub use jsonrpc::{
    JSONRPC_VERSION, JsonRpcErrorObject, JsonRpcErrorResponse, JsonRpcRequest, JsonRpcResponse,
    JsonRpcSuccess,
};
pub use lifecycle::{
    ApprovalSurface, BackendKind, Channel, Curve, LockState, MessageFormat, RejectReasonCode,
    RequestKind, RequestStatus, ResultKind, TransportKind, WalletCapability,
};
pub use native_bridge::{
    NATIVE_BRIDGE_MAX_INBOUND_BYTES, NATIVE_BRIDGE_MAX_OUTBOUND_BYTES,
    NATIVE_BRIDGE_PROTOCOL_VERSION, NativeBridgeAccount, NativeBridgeRequest, NativeBridgeResponse,
};
pub use payload::{MessagePayload, RequestPayload, RequestResult, TransactionPayload};
pub use records::{
    DeliveryLease, PresentationLease, RequestRecord, WalletAccountRecord, WalletInstanceRecord,
};
pub use time::{DurationSeconds, TimestampMs};

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{ClientRequestId, RequestKind, RequestStatus, RequestStatus::Created, ResultKind};

    #[test]
    fn request_status_marks_terminal_states() {
        assert_eq!(Created.is_terminal(), false);
        assert_eq!(RequestStatus::Approved.is_terminal(), true);
    }

    #[test]
    fn request_kind_maps_to_result_kind() {
        assert_eq!(
            RequestKind::SignMessage.expected_result_kind(),
            ResultKind::SignedMessage
        );
    }

    #[test]
    fn ids_reject_empty_values() {
        let result = ClientRequestId::new("   ");
        assert!(result.is_err());
    }
}
