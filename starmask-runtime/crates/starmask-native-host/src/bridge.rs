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
                    message_format: request.message_format,
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
            created_account_address,
            created_account_public_key,
            created_account_curve,
            created_account_is_default,
            created_account_is_locked,
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
                created_account_address,
                created_account_public_key,
                created_account_curve,
                created_account_is_default,
                created_account_is_locked,
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
    use std::cell::RefCell;

    use starmask_types::{
        AckResult, ClientRequestId, Curve, GetRequestStatusParams, GetRequestStatusResult,
        LockState, PayloadHash, RequestHasAvailableParams, RequestHasAvailableResult, RequestId,
        RequestKind, RequestPullNextResult, RequestRejectParams, RequestResolveParams, ResultKind,
        SharedError, SharedErrorCode, WalletInstanceId,
    };

    use super::*;

    #[derive(Default)]
    struct FakeClient {
        pull_next_result: Option<Result<RequestPullNextResult, SharedError>>,
        heartbeat_result: Option<Result<AckResult, SharedError>>,
        resolve_calls: RefCell<Vec<RequestResolveParams>>,
        reject_calls: RefCell<Vec<RequestRejectParams>>,
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
            self.heartbeat_result
                .clone()
                .unwrap_or(Ok(AckResult { ok: true }))
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

        fn request_resolve(&self, params: RequestResolveParams) -> Result<AckResult, SharedError> {
            self.resolve_calls.borrow_mut().push(params);
            Ok(AckResult { ok: true })
        }

        fn request_reject(&self, params: RequestRejectParams) -> Result<AckResult, SharedError> {
            self.reject_calls.borrow_mut().push(params);
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
                    message_format: None,
                }),
            })),
            ..Default::default()
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
                message_format: None,
            }
        );
    }

    #[test]
    fn pull_next_without_request_maps_to_request_none() {
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let client = FakeClient {
            pull_next_result: Some(Ok(RequestPullNextResult {
                wallet_instance_id: wallet_instance_id.clone(),
                request: None,
            })),
            ..Default::default()
        };

        let response = handle_request(
            &client,
            NativeBridgeRequest::RequestPullNext {
                message_id: "msg-none".to_owned(),
                wallet_instance_id: wallet_instance_id.clone(),
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::RequestNone {
                reply_to: "msg-none".to_owned(),
                wallet_instance_id,
            }
        );
    }

    #[test]
    fn daemon_error_is_reported_as_extension_error_with_reply_to() {
        let client = FakeClient {
            heartbeat_result: Some(Err(SharedError::new(
                SharedErrorCode::WalletUnavailable,
                "wallet offline",
            )
            .with_retryable(false))),
            ..Default::default()
        };

        let response = handle_request(
            &client,
            NativeBridgeRequest::ExtensionHeartbeat {
                message_id: "msg-heartbeat".to_owned(),
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                presented_request_ids: Vec::new(),
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::ExtensionError {
                reply_to: Some("msg-heartbeat".to_owned()),
                code: SharedErrorCode::WalletUnavailable,
                message: "wallet offline".to_owned(),
                retryable: Some(false),
            }
        );
    }

    #[test]
    fn request_resolve_forwards_signature_payload_to_daemon() {
        let client = FakeClient::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let request_id = RequestId::new("request-1").unwrap();
        let presentation_id = starmask_types::PresentationId::new("presentation-1").unwrap();

        let response = handle_request(
            &client,
            NativeBridgeRequest::RequestResolve {
                message_id: "msg-resolve".to_owned(),
                wallet_instance_id: wallet_instance_id.clone(),
                request_id: request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::SignedMessage,
                signed_txn_bcs_hex: None,
                signature: Some("0xsig".to_owned()),
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::ExtensionAck {
                reply_to: "msg-resolve".to_owned(),
            }
        );
        assert_eq!(client.resolve_calls.borrow().len(), 1);
        assert_eq!(
            client.resolve_calls.borrow()[0],
            RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                request_id,
                presentation_id,
                result_kind: ResultKind::SignedMessage,
                signed_txn_bcs_hex: None,
                signature: Some("0xsig".to_owned()),
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
            }
        );
    }

    #[test]
    fn request_resolve_forwards_created_account_payload_to_daemon() {
        let client = FakeClient::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let request_id = RequestId::new("request-1").unwrap();
        let presentation_id = starmask_types::PresentationId::new("presentation-1").unwrap();

        let response = handle_request(
            &client,
            NativeBridgeRequest::RequestResolve {
                message_id: "msg-resolve-created".to_owned(),
                wallet_instance_id: wallet_instance_id.clone(),
                request_id: request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::CreatedAccount,
                signed_txn_bcs_hex: None,
                signature: None,
                created_account_address: Some("0xabc".to_owned()),
                created_account_public_key: Some("0xpub".to_owned()),
                created_account_curve: Some(Curve::Ed25519),
                created_account_is_default: Some(true),
                created_account_is_locked: Some(false),
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::ExtensionAck {
                reply_to: "msg-resolve-created".to_owned(),
            }
        );
        assert_eq!(client.resolve_calls.borrow().len(), 1);
        assert_eq!(
            client.resolve_calls.borrow()[0],
            RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                request_id,
                presentation_id,
                result_kind: ResultKind::CreatedAccount,
                signed_txn_bcs_hex: None,
                signature: None,
                created_account_address: Some("0xabc".to_owned()),
                created_account_public_key: Some("0xpub".to_owned()),
                created_account_curve: Some(Curve::Ed25519),
                created_account_is_default: Some(true),
                created_account_is_locked: Some(false),
            }
        );
    }

    #[test]
    fn request_reject_forwards_reason_code_to_daemon() {
        let client = FakeClient::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let request_id = RequestId::new("request-1").unwrap();
        let presentation_id = starmask_types::PresentationId::new("presentation-1").unwrap();

        let response = handle_request(
            &client,
            NativeBridgeRequest::RequestReject {
                message_id: "msg-reject".to_owned(),
                wallet_instance_id: wallet_instance_id.clone(),
                request_id: request_id.clone(),
                presentation_id: Some(presentation_id.clone()),
                reason_code: starmask_types::RejectReasonCode::RequestRejected,
                reason_message: Some("nope".to_owned()),
            },
        );

        assert_eq!(
            response,
            NativeBridgeResponse::ExtensionAck {
                reply_to: "msg-reject".to_owned(),
            }
        );
        assert_eq!(client.reject_calls.borrow().len(), 1);
        assert_eq!(
            client.reject_calls.borrow()[0],
            RequestRejectParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id,
                request_id,
                presentation_id: Some(presentation_id),
                reason_code: starmask_types::RejectReasonCode::RequestRejected,
                reason_message: Some("nope".to_owned()),
            }
        );
    }
}
