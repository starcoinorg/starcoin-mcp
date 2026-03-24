use starmask_types::{
    ExtensionHeartbeatParams, ExtensionRegisterParams, ExtensionUpdateAccountsParams,
    NATIVE_BRIDGE_PROTOCOL_VERSION, NativeBridgeRequest, NativeBridgeResponse,
    RequestPresentedParams, RequestPullNextParams, RequestRejectParams, RequestResolveParams,
    SharedError, SharedErrorCode,
};

use crate::client::{DaemonRpc, daemon_protocol_version};

pub fn handle_request<D>(client: &D, request: NativeBridgeRequest) -> NativeBridgeResponse
where
    D: DaemonRpc,
{
    let reply_to = request.message_id().to_owned();
    let result = match request {
        NativeBridgeRequest::ExtensionRegister {
            protocol_version,
            wallet_instance_id,
            extension_id,
            extension_version,
            profile_hint,
            lock_state,
            accounts_summary,
            ..
        } => {
            if protocol_version != NATIVE_BRIDGE_PROTOCOL_VERSION {
                return NativeBridgeResponse::ExtensionError {
                    reply_to: Some(reply_to),
                    code: SharedErrorCode::ProtocolVersionMismatch,
                    message: format!(
                        "Unsupported native bridge protocol version {protocol_version}"
                    ),
                    retryable: Some(false),
                };
            }
            client
                .extension_register(ExtensionRegisterParams {
                    protocol_version: daemon_protocol_version(),
                    wallet_instance_id,
                    extension_id,
                    extension_version,
                    profile_hint,
                    lock_state,
                    accounts_summary,
                })
                .map(|result| NativeBridgeResponse::ExtensionRegistered {
                    reply_to: reply_to.clone(),
                    wallet_instance_id: result.wallet_instance_id,
                    daemon_protocol_version: result.daemon_protocol_version,
                    accepted: result.accepted,
                })
        }
        NativeBridgeRequest::ExtensionHeartbeat {
            wallet_instance_id,
            presented_request_ids,
            ..
        } => client
            .extension_heartbeat(ExtensionHeartbeatParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                presented_request_ids,
            })
            .map(|_| NativeBridgeResponse::ExtensionAck {
                reply_to: reply_to.clone(),
            }),
        NativeBridgeRequest::ExtensionUpdateAccounts {
            wallet_instance_id,
            lock_state,
            accounts,
            ..
        } => client
            .extension_update_accounts(ExtensionUpdateAccountsParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                lock_state,
                accounts,
            })
            .map(|_| NativeBridgeResponse::ExtensionAck {
                reply_to: reply_to.clone(),
            }),
        NativeBridgeRequest::RequestPullNext {
            wallet_instance_id, ..
        } => client
            .request_pull_next(RequestPullNextParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .map(|result| match result.request {
                Some(request) => NativeBridgeResponse::RequestNext {
                    reply_to: reply_to.clone(),
                    request_id: request.request_id,
                    client_request_id: request.client_request_id,
                    kind: request.kind,
                    account_address: request.account_address,
                    payload_hash: request.payload_hash,
                    display_hint: request.display_hint,
                    client_context: request.client_context,
                    resume_required: request.resume_required,
                    delivery_lease_id: request.delivery_lease_id,
                    lease_expires_at: request.lease_expires_at,
                    presentation_id: request.presentation_id,
                    presentation_expires_at: request.presentation_expires_at,
                    raw_txn_bcs_hex: request.raw_txn_bcs_hex,
                    message: request.message,
                },
                None => NativeBridgeResponse::RequestNone {
                    reply_to: reply_to.clone(),
                    wallet_instance_id,
                },
            }),
        NativeBridgeRequest::RequestPresented {
            wallet_instance_id,
            request_id,
            delivery_lease_id,
            presentation_id,
            ..
        } => client
            .request_presented(RequestPresentedParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                request_id,
                delivery_lease_id,
                presentation_id,
            })
            .map(|_| NativeBridgeResponse::ExtensionAck {
                reply_to: reply_to.clone(),
            }),
        NativeBridgeRequest::RequestResolve {
            wallet_instance_id,
            request_id,
            presentation_id,
            result_kind,
            signed_txn_bcs_hex,
            signature,
            ..
        } => client
            .request_resolve(RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                request_id,
                presentation_id,
                result_kind,
                signed_txn_bcs_hex,
                signature,
            })
            .map(|_| NativeBridgeResponse::ExtensionAck {
                reply_to: reply_to.clone(),
            }),
        NativeBridgeRequest::RequestReject {
            wallet_instance_id,
            request_id,
            presentation_id,
            reason_code,
            reason_message,
            ..
        } => client
            .request_reject(RequestRejectParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                request_id,
                presentation_id,
                reason_code,
                reason_message,
            })
            .map(|_| NativeBridgeResponse::ExtensionAck {
                reply_to: reply_to.clone(),
            }),
    };

    result.unwrap_or_else(|error| shared_error_response(Some(reply_to), error))
}

pub fn shared_error_response(reply_to: Option<String>, error: SharedError) -> NativeBridgeResponse {
    NativeBridgeResponse::ExtensionError {
        reply_to,
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

#[cfg(test)]
mod tests {
    use starmask_types::{
        AckResult, ClientRequestId, GetRequestStatusParams, GetRequestStatusResult, LockState,
        PayloadHash, RequestHasAvailableParams, RequestHasAvailableResult, RequestId, RequestKind,
        RequestPullNextResult, SharedError, SharedErrorCode, WalletInstanceId,
    };

    use super::*;

    #[derive(Default)]
    struct FakeClient {
        pull_next_result: Option<Result<RequestPullNextResult, SharedError>>,
    }

    impl DaemonRpc for FakeClient {
        fn extension_register(
            &self,
            _params: ExtensionRegisterParams,
        ) -> Result<starmask_types::ExtensionRegisteredResult, SharedError> {
            Ok(starmask_types::ExtensionRegisteredResult {
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                daemon_protocol_version: 1,
                accepted: true,
            })
        }

        fn extension_heartbeat(
            &self,
            _params: ExtensionHeartbeatParams,
        ) -> Result<AckResult, SharedError> {
            Ok(AckResult { ok: true })
        }

        fn extension_update_accounts(
            &self,
            _params: ExtensionUpdateAccountsParams,
        ) -> Result<AckResult, SharedError> {
            Ok(AckResult { ok: true })
        }

        fn request_pull_next(
            &self,
            _params: RequestPullNextParams,
        ) -> Result<RequestPullNextResult, SharedError> {
            self.pull_next_result.clone().unwrap()
        }

        fn request_has_available(
            &self,
            _params: RequestHasAvailableParams,
        ) -> Result<RequestHasAvailableResult, SharedError> {
            Ok(RequestHasAvailableResult {
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                available: false,
            })
        }

        fn get_request_status(
            &self,
            _params: GetRequestStatusParams,
        ) -> Result<GetRequestStatusResult, SharedError> {
            panic!("get_request_status should not be called in these tests")
        }

        fn request_presented(
            &self,
            _params: RequestPresentedParams,
        ) -> Result<AckResult, SharedError> {
            Ok(AckResult { ok: true })
        }

        fn request_resolve(&self, _params: RequestResolveParams) -> Result<AckResult, SharedError> {
            Ok(AckResult { ok: true })
        }

        fn request_reject(&self, _params: RequestRejectParams) -> Result<AckResult, SharedError> {
            Ok(AckResult { ok: true })
        }
    }

    #[test]
    fn register_rejects_protocol_mismatch_before_daemon_call() {
        let client = FakeClient::default();
        let response = handle_request(
            &client,
            NativeBridgeRequest::ExtensionRegister {
                message_id: "msg-1".to_owned(),
                protocol_version: 99,
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                extension_id: "ext".to_owned(),
                extension_version: "1.0.0".to_owned(),
                profile_hint: None,
                lock_state: LockState::Unlocked,
                accounts_summary: vec![],
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::ExtensionError {
                reply_to: Some("msg-1".to_owned()),
                code: SharedErrorCode::ProtocolVersionMismatch,
                message: "Unsupported native bridge protocol version 99".to_owned(),
                retryable: Some(false),
            }
        );
    }

    #[test]
    fn pull_next_maps_result_to_request_next() {
        let client = FakeClient {
            pull_next_result: Some(Ok(RequestPullNextResult {
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                request: Some(starmask_types::PulledRequest {
                    request_id: RequestId::new("req-1").unwrap(),
                    client_request_id: ClientRequestId::new("client-1").unwrap(),
                    kind: RequestKind::SignTransaction,
                    account_address: "0x1".to_owned(),
                    payload_hash: PayloadHash::new("hash-1").unwrap(),
                    display_hint: Some("Transfer".to_owned()),
                    client_context: Some("codex".to_owned()),
                    resume_required: false,
                    delivery_lease_id: Some(
                        starmask_types::DeliveryLeaseId::new("lease-1").unwrap(),
                    ),
                    lease_expires_at: Some(starmask_types::TimestampMs::from_millis(42)),
                    presentation_id: None,
                    presentation_expires_at: None,
                    raw_txn_bcs_hex: Some("0xabc".to_owned()),
                    message: None,
                }),
            })),
        };

        let response = handle_request(
            &client,
            NativeBridgeRequest::RequestPullNext {
                message_id: "msg-2".to_owned(),
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::RequestNext {
                reply_to: "msg-2".to_owned(),
                request_id: RequestId::new("req-1").unwrap(),
                client_request_id: ClientRequestId::new("client-1").unwrap(),
                kind: RequestKind::SignTransaction,
                account_address: "0x1".to_owned(),
                payload_hash: PayloadHash::new("hash-1").unwrap(),
                display_hint: Some("Transfer".to_owned()),
                client_context: Some("codex".to_owned()),
                resume_required: false,
                delivery_lease_id: Some(starmask_types::DeliveryLeaseId::new("lease-1").unwrap()),
                lease_expires_at: Some(starmask_types::TimestampMs::from_millis(42)),
                presentation_id: None,
                presentation_expires_at: None,
                raw_txn_bcs_hex: Some("0xabc".to_owned()),
                message: None,
            }
        );
    }
}
