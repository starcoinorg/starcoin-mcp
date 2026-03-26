use std::{collections::BTreeSet, path::Path, time::Duration};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    time::timeout,
};
use tracing::{debug, warn};

use starmask_core::{
    CoordinatorCommand, CoordinatorResponse,
    commands::{
        CreateSignMessageCommand, CreateSignTransactionCommand, HeartbeatExtensionCommand,
        MarkRequestPresentedCommand, RegisterExtensionCommand, RejectRequestCommand,
        ResolveRequestCommand, UpdateExtensionAccountsCommand,
    },
};
use starmask_types::{
    AckResult, CancelRequestParams, Channel, CreateSignMessageParams, CreateSignTransactionParams,
    ExtensionHeartbeatParams, ExtensionRegisterParams, ExtensionRegisteredResult,
    ExtensionUpdateAccountsParams, GetRequestStatusParams, JsonRpcErrorResponse, JsonRpcRequest,
    JsonRpcResponse, JsonRpcSuccess, NativeBridgeAccount, RequestHasAvailableParams,
    RequestPresentedParams, RequestPullNextParams, RequestRejectParams, RequestResolveParams,
    RequestResult, ResultKind, SharedError, SharedErrorCode, SystemGetInfoParams, SystemPingParams,
    TimestampMs, WalletAccountRecord, WalletGetPublicKeyParams, WalletListAccountsParams,
    WalletListInstancesParams, WalletStatusParams,
};

use crate::coordinator_runtime::CoordinatorHandle;

const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ServerPolicy {
    pub channel: Channel,
    pub allowed_extension_ids: BTreeSet<String>,
    pub native_host_name: String,
}

impl ServerPolicy {
    fn accepts_extension(&self, extension_id: &str) -> bool {
        self.allowed_extension_ids.contains(extension_id)
    }
}

pub async fn run_unix_server(
    socket_path: &Path,
    handle: CoordinatorHandle,
    policy: ServerPolicy,
) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove {}", socket_path.display()))?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to lock down {}", parent.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to lock down {}", socket_path.display()))?;
    loop {
        let (stream, _) = listener.accept().await?;
        let handle = handle.clone();
        let policy = policy.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, handle, policy).await {
                warn!(%error, "failed to handle daemon rpc connection");
            }
        });
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    handle: CoordinatorHandle,
    policy: ServerPolicy,
) -> Result<()> {
    let bytes = read_request_bytes(&mut stream).await?;
    if bytes.is_empty() {
        return Ok(());
    }

    let request: JsonRpcRequest<Value> = serde_json::from_slice(&bytes)?;
    debug!(method = %request.method, id = %request.id, "received daemon rpc request");
    let response = match dispatch_request(request, handle, policy).await {
        Ok(response) => response,
        Err(response) => JsonRpcResponse::Error(response),
    };
    let encoded = serde_json::to_vec(&response)?;
    stream.write_all(&encoded).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn dispatch_request(
    request: JsonRpcRequest<Value>,
    handle: CoordinatorHandle,
    policy: ServerPolicy,
) -> Result<JsonRpcResponse<Value>, JsonRpcErrorResponse> {
    let id = request.id.clone();
    let result = match request.method.as_str() {
        "system.ping" => {
            decode_protocol::<SystemPingParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle.dispatch(CoordinatorCommand::SystemPing).await,
                |response| match response {
                    CoordinatorResponse::SystemPing(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "system.getInfo" => {
            decode_protocol::<SystemGetInfoParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle.dispatch(CoordinatorCommand::SystemGetInfo).await,
                |response| match response {
                    CoordinatorResponse::SystemInfo(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "wallet.status" => {
            decode_protocol::<WalletStatusParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle.dispatch(CoordinatorCommand::WalletStatus).await,
                |response| match response {
                    CoordinatorResponse::WalletStatus(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "wallet.listInstances" => {
            let params = decode_protocol::<WalletListInstancesParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::WalletListInstances {
                        connected_only: params.connected_only,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::WalletInstances(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "wallet.listAccounts" => {
            let params = decode_protocol::<WalletListAccountsParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::WalletListAccounts {
                        wallet_instance_id: params.wallet_instance_id,
                        include_public_key: params.include_public_key,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::WalletAccounts(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "wallet.getPublicKey" => {
            let params = decode_protocol::<WalletGetPublicKeyParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::WalletGetPublicKey {
                        address: params.address,
                        wallet_instance_id: params.wallet_instance_id,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::WalletPublicKey(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.createSignTransaction" => {
            let params = decode_protocol::<CreateSignTransactionParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::CreateSignTransaction(
                        CreateSignTransactionCommand {
                            client_request_id: params.client_request_id,
                            account_address: params.account_address,
                            wallet_instance_id: params.wallet_instance_id,
                            chain_id: params.chain_id,
                            raw_txn_bcs_hex: params.raw_txn_bcs_hex,
                            tx_kind: params.tx_kind,
                            display_hint: params.display_hint,
                            client_context: params.client_context,
                            ttl_seconds: params.ttl_seconds,
                        },
                    ))
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestCreated(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.createSignMessage" => {
            let params = decode_protocol::<CreateSignMessageParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::CreateSignMessage(
                        CreateSignMessageCommand {
                            client_request_id: params.client_request_id,
                            account_address: params.account_address,
                            wallet_instance_id: params.wallet_instance_id,
                            message: params.message,
                            format: params.format,
                            display_hint: params.display_hint,
                            client_context: params.client_context,
                            ttl_seconds: params.ttl_seconds,
                        },
                    ))
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestCreated(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.getStatus" => {
            let params = decode_protocol::<GetRequestStatusParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::GetRequestStatus {
                        request_id: params.request_id,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestStatus(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.hasAvailable" => {
            let params = decode_protocol::<RequestHasAvailableParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::RequestHasAvailable {
                        wallet_instance_id: params.wallet_instance_id,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestHasAvailable(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.cancel" => {
            let params = decode_protocol::<CancelRequestParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::CancelRequest {
                        request_id: params.request_id,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestCancelled(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "extension.register" => {
            let params = decode_protocol::<ExtensionRegisterParams>(&id, &request.params)?;
            if !policy.accepts_extension(&params.extension_id) {
                warn!(
                    channel = ?policy.channel,
                    native_host_name = %policy.native_host_name,
                    extension_id = %params.extension_id,
                    "rejected extension registration outside allowlist"
                );
                serde_json::to_value(ExtensionRegisteredResult {
                    wallet_instance_id: params.wallet_instance_id,
                    daemon_protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                    accepted: false,
                })
                .map_err(|error| error_response(None, error))?
            } else {
                let wallet_instance_id = params.wallet_instance_id.clone();
                let lock_state = params.lock_state;
                expect_response(
                    handle
                        .dispatch(CoordinatorCommand::RegisterExtension(
                            RegisterExtensionCommand {
                                wallet_instance_id: wallet_instance_id.clone(),
                                extension_id: params.extension_id,
                                extension_version: params.extension_version,
                                protocol_version: params.protocol_version,
                                profile_hint: params.profile_hint,
                                lock_state,
                                accounts: params
                                    .accounts_summary
                                    .into_iter()
                                    .map(|account| {
                                        bridge_account_to_wallet_account(
                                            &wallet_instance_id,
                                            account,
                                            lock_state,
                                        )
                                    })
                                    .collect(),
                            },
                        ))
                        .await,
                    |response| match response {
                        CoordinatorResponse::Ack => Ok(()),
                        other => Err(unexpected_response(other)),
                    },
                )?;
                serde_json::to_value(ExtensionRegisteredResult {
                    wallet_instance_id,
                    daemon_protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                    accepted: true,
                })
                .map_err(|error| error_response(None, error))?
            }
        }
        "extension.heartbeat" => {
            let params = decode_protocol::<ExtensionHeartbeatParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::HeartbeatExtension(
                        HeartbeatExtensionCommand {
                            wallet_instance_id: params.wallet_instance_id,
                            presented_request_ids: params.presented_request_ids,
                        },
                    ))
                    .await,
                |response| match response {
                    CoordinatorResponse::Ack => Ok(AckResult { ok: true }),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "extension.updateAccounts" => {
            let params = decode_protocol::<ExtensionUpdateAccountsParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                        UpdateExtensionAccountsCommand {
                            wallet_instance_id: params.wallet_instance_id.clone(),
                            lock_state: params.lock_state,
                            accounts: params
                                .accounts
                                .into_iter()
                                .map(|account| {
                                    bridge_account_to_wallet_account(
                                        &params.wallet_instance_id,
                                        account,
                                        params.lock_state,
                                    )
                                })
                                .collect(),
                        },
                    ))
                    .await,
                |response| match response {
                    CoordinatorResponse::Ack => Ok(AckResult { ok: true }),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.pullNext" => {
            let params = decode_protocol::<RequestPullNextParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::PullNextRequest {
                        wallet_instance_id: params.wallet_instance_id,
                    })
                    .await,
                |response| match response {
                    CoordinatorResponse::PullNextRequest(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.presented" => {
            let params = decode_protocol::<RequestPresentedParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::MarkRequestPresented(
                        MarkRequestPresentedCommand {
                            request_id: params.request_id,
                            wallet_instance_id: params.wallet_instance_id,
                            delivery_lease_id: params.delivery_lease_id,
                            presentation_id: params.presentation_id,
                        },
                    ))
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestPresented(_) => Ok(AckResult { ok: true }),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.resolve" => {
            let params = decode_protocol::<RequestResolveParams>(&id, &request.params)?;
            let result = request_result_from_params(&params)
                .map_err(|error| error_response(Some(&id), error))?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::ResolveRequest(ResolveRequestCommand {
                        request_id: params.request_id,
                        wallet_instance_id: params.wallet_instance_id,
                        presentation_id: params.presentation_id,
                        result,
                    }))
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestResolved(_) => Ok(AckResult { ok: true }),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.reject" => {
            let params = decode_protocol::<RequestRejectParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::RejectRequest(RejectRequestCommand {
                        request_id: params.request_id,
                        wallet_instance_id: params.wallet_instance_id,
                        presentation_id: params.presentation_id,
                        reason_code: params.reason_code,
                        message: params.reason_message,
                    }))
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestRejected(_) => Ok(AckResult { ok: true }),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        other => {
            return Err(JsonRpcErrorResponse::new(
                id,
                SharedError::new(
                    SharedErrorCode::UnsupportedOperation,
                    format!("Unsupported daemon method: {other}"),
                )
                .with_retryable(false),
            ));
        }
    };

    Ok(JsonRpcResponse::Success(JsonRpcSuccess::new(id, result)))
}

fn decode_protocol<T>(id: &str, value: &Value) -> Result<T, JsonRpcErrorResponse>
where
    T: DeserializeOwned + HasProtocolVersion,
{
    let params = serde_json::from_value::<T>(value.clone())
        .map_err(|error| error_response(Some(id), error))?;
    if params.protocol_version() != starmask_types::DAEMON_PROTOCOL_VERSION {
        return Err(JsonRpcErrorResponse::new(
            id,
            SharedError::new(
                SharedErrorCode::ProtocolVersionMismatch,
                format!(
                    "Unsupported daemon protocol version {}",
                    params.protocol_version()
                ),
            ),
        ));
    }
    Ok(params)
}

async fn read_request_bytes(stream: &mut UnixStream) -> Result<Vec<u8>> {
    timeout(REQUEST_READ_TIMEOUT, async {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            let next_len = bytes
                .len()
                .checked_add(read)
                .ok_or_else(|| anyhow!("daemon rpc request size overflow"))?;
            if next_len > MAX_REQUEST_BYTES {
                bail!("daemon rpc request exceeds {MAX_REQUEST_BYTES} bytes");
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        Ok::<_, anyhow::Error>(bytes)
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for daemon rpc request"))?
}

trait HasProtocolVersion {
    fn protocol_version(&self) -> u32;
}

macro_rules! impl_has_protocol_version {
    ($($name:ty),+ $(,)?) => {
        $(impl HasProtocolVersion for $name {
            fn protocol_version(&self) -> u32 {
                self.protocol_version
            }
        })+
    };
}

impl_has_protocol_version!(
    CancelRequestParams,
    CreateSignMessageParams,
    CreateSignTransactionParams,
    ExtensionHeartbeatParams,
    ExtensionRegisterParams,
    ExtensionUpdateAccountsParams,
    GetRequestStatusParams,
    RequestHasAvailableParams,
    RequestPresentedParams,
    RequestPullNextParams,
    RequestRejectParams,
    RequestResolveParams,
    SystemGetInfoParams,
    SystemPingParams,
    WalletGetPublicKeyParams,
    WalletListAccountsParams,
    WalletListInstancesParams,
    WalletStatusParams,
);

fn expect_response<T, F>(
    result: Result<CoordinatorResponse>,
    projector: F,
) -> Result<T, JsonRpcErrorResponse>
where
    F: FnOnce(CoordinatorResponse) -> Result<T, JsonRpcErrorResponse>,
{
    let response = result.map_err(map_runtime_error)?;
    projector(response)
}

fn unexpected_response(response: CoordinatorResponse) -> JsonRpcErrorResponse {
    JsonRpcErrorResponse::new(
        "",
        SharedError::new(
            SharedErrorCode::InternalBridgeError,
            format!("unexpected coordinator response: {response:?}"),
        ),
    )
}

fn map_runtime_error(error: anyhow::Error) -> JsonRpcErrorResponse {
    if let Some(shared) = extract_shared_error(&error) {
        return JsonRpcErrorResponse::new("", shared);
    }
    JsonRpcErrorResponse::new(
        "",
        SharedError::new(SharedErrorCode::InternalBridgeError, error.to_string()),
    )
}

fn error_response(id: Option<&str>, error: impl std::fmt::Display) -> JsonRpcErrorResponse {
    JsonRpcErrorResponse::new(
        id.unwrap_or(""),
        SharedError::new(SharedErrorCode::InternalBridgeError, error.to_string()),
    )
}

fn extract_shared_error(error: &anyhow::Error) -> Option<SharedError> {
    if let Some(shared) = error.downcast_ref::<SharedError>() {
        return Some(shared.clone());
    }
    if let Some(starmask_core::CoreError::Shared(shared)) =
        error.downcast_ref::<starmask_core::CoreError>()
    {
        return Some(shared.clone());
    }
    None
}

fn request_result_from_params(params: &RequestResolveParams) -> Result<RequestResult> {
    match params.result_kind {
        ResultKind::SignedTransaction => {
            let signed_txn_bcs_hex = params
                .signed_txn_bcs_hex
                .clone()
                .context("signed_txn_bcs_hex is required for signed_transaction")?;
            Ok(RequestResult::SignedTransaction { signed_txn_bcs_hex })
        }
        ResultKind::SignedMessage => {
            let signature = params
                .signature
                .clone()
                .context("signature is required for signed_message")?;
            Ok(RequestResult::SignedMessage { signature })
        }
        ResultKind::None => anyhow::bail!("result_kind none is not valid for request.resolve"),
    }
}

fn bridge_account_to_wallet_account(
    wallet_instance_id: &starmask_types::WalletInstanceId,
    account: NativeBridgeAccount,
    lock_state: starmask_types::LockState,
) -> WalletAccountRecord {
    WalletAccountRecord {
        wallet_instance_id: wallet_instance_id.clone(),
        address: account.address,
        label: account.label,
        public_key: account.public_key,
        is_default: account.is_default,
        is_locked: lock_state != starmask_types::LockState::Unlocked,
        last_seen_at: TimestampMs::from_millis(0),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::{ServerPolicy, dispatch_request};
    use crate::coordinator_runtime::CoordinatorHandle;
    use starmask_types::{
        Channel, DAEMON_PROTOCOL_VERSION, ExtensionRegisteredResult, JsonRpcRequest,
        JsonRpcResponse, JsonRpcSuccess, LockState, SharedErrorCode, WalletInstanceId,
    };

    fn test_policy(allowed_extension_ids: &[&str]) -> ServerPolicy {
        ServerPolicy {
            channel: Channel::Development,
            allowed_extension_ids: allowed_extension_ids
                .iter()
                .map(|extension_id| (*extension_id).to_owned())
                .collect(),
            native_host_name: "com.starcoin.test".to_owned(),
        }
    }

    fn test_handle() -> CoordinatorHandle {
        CoordinatorHandle::closed_for_tests()
    }

    fn wallet_instance_id() -> WalletInstanceId {
        WalletInstanceId::new("wallet-1").expect("wallet instance id should be valid")
    }

    fn request(id: &str, method: &str, params: impl Into<Value>) -> JsonRpcRequest<Value> {
        JsonRpcRequest::new(id, method, params.into())
    }

    #[tokio::test]
    async fn rejects_protocol_version_mismatch_before_dispatch() {
        let error = dispatch_request(
            request(
                "req-1",
                "system.ping",
                json!({
                    "protocol_version": DAEMON_PROTOCOL_VERSION + 1,
                }),
            ),
            test_handle(),
            test_policy(&[]),
        )
        .await
        .expect_err("protocol mismatch should fail before dispatch");

        assert_eq!(error.id, "req-1");
        assert_eq!(error.error.code, SharedErrorCode::ProtocolVersionMismatch);
        assert_eq!(error.error.retryable, Some(false));
        assert_eq!(
            error.error.message,
            format!(
                "Unsupported daemon protocol version {}",
                DAEMON_PROTOCOL_VERSION + 1
            )
        );
    }

    #[tokio::test]
    async fn rejects_unknown_method_without_dispatch() {
        let error = dispatch_request(
            request("req-2", "wallet.notReal", json!({})),
            test_handle(),
            test_policy(&[]),
        )
        .await
        .expect_err("unsupported method should fail");

        assert_eq!(error.id, "req-2");
        assert_eq!(error.error.code, SharedErrorCode::UnsupportedOperation);
        assert_eq!(error.error.retryable, Some(false));
        assert_eq!(
            error.error.message,
            "Unsupported daemon method: wallet.notReal"
        );
    }

    #[tokio::test]
    async fn rejects_request_resolve_without_required_signature() {
        let error = dispatch_request(
            request(
                "req-3",
                "request.resolve",
                json!({
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "wallet_instance_id": wallet_instance_id(),
                    "request_id": "request-1",
                    "presentation_id": "presentation-1",
                    "result_kind": "signed_message",
                }),
            ),
            test_handle(),
            test_policy(&[]),
        )
        .await
        .expect_err("missing signature should fail before dispatch");

        assert_eq!(error.id, "req-3");
        assert_eq!(error.error.code, SharedErrorCode::InternalBridgeError);
        assert_eq!(error.error.retryable, Some(true));
        assert_eq!(
            error.error.message,
            "signature is required for signed_message"
        );
    }

    #[test]
    fn runtime_shared_error_preserves_original_code() {
        let error =
            super::map_runtime_error(anyhow::Error::from(starmask_core::CoreError::shared(
                SharedErrorCode::IdempotencyKeyConflict,
                "duplicate client_request_id",
            )));

        assert_eq!(error.error.code, SharedErrorCode::IdempotencyKeyConflict);
        assert_eq!(error.error.message, "duplicate client_request_id");
        assert_eq!(error.error.retryable, Some(false));
    }

    #[tokio::test]
    async fn rejects_extension_registration_outside_allowlist_without_dispatch() {
        let response = dispatch_request(
            request(
                "req-4",
                "extension.register",
                json!({
                    "protocol_version": DAEMON_PROTOCOL_VERSION,
                    "wallet_instance_id": wallet_instance_id(),
                    "extension_id": "blocked-extension",
                    "extension_version": "1.2.3",
                    "profile_hint": "default",
                    "lock_state": LockState::Unlocked,
                    "accounts_summary": [],
                }),
            ),
            test_handle(),
            test_policy(&["allowed-extension"]),
        )
        .await
        .expect("blocked extension should return accepted=false");

        let JsonRpcResponse::Success(JsonRpcSuccess { id, result, .. }) = response else {
            panic!("expected success response");
        };
        let result: ExtensionRegisteredResult =
            serde_json::from_value(result).expect("extension register result should decode");

        assert_eq!(id, "req-4");
        assert_eq!(
            result,
            ExtensionRegisteredResult {
                wallet_instance_id: wallet_instance_id(),
                daemon_protocol_version: DAEMON_PROTOCOL_VERSION,
                accepted: false,
            }
        );
    }
}
