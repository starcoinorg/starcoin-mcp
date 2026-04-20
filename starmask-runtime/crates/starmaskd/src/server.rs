use std::{path::Path, time::Duration};

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
        CreateAccountCommand, CreateExportAccountCommand, CreateImportAccountCommand,
        CreateSignMessageCommand, CreateSignTransactionCommand, HeartbeatBackendCommand,
        HeartbeatExtensionCommand, MarkRequestPresentedCommand, RegisterBackendCommand,
        RejectRequestCommand, ResolveRequestCommand, SetAccountLabelCommand,
        UpdateBackendAccountsCommand, UpdateExtensionAccountsCommand,
    },
};
use starmask_types::{
    AckResult, BackendHeartbeatParams, BackendRegisterParams, BackendRegisteredResult,
    BackendUpdateAccountsParams, CancelRequestParams, Channel, CreateAccountParams,
    CreateExportAccountParams, CreateImportAccountParams, CreateSignMessageParams,
    CreateSignTransactionParams, DAEMON_PROTOCOL_VERSION, ExtensionHeartbeatParams,
    ExtensionRegisterParams, ExtensionRegisteredResult, ExtensionUpdateAccountsParams,
    GENERIC_BACKEND_PROTOCOL_VERSION, GetRequestStatusParams, JsonRpcErrorResponse, JsonRpcRequest,
    JsonRpcResponse, JsonRpcSuccess, NativeBridgeAccount, RequestHasAvailableParams,
    RequestPresentedParams, RequestPullNextParams, RequestRejectParams, RequestResolveParams,
    RequestResult, ResultKind, SharedError, SharedErrorCode, SystemGetInfoParams, SystemPingParams,
    TimestampMs, WalletAccountRecord, WalletCapability, WalletGetPublicKeyParams,
    WalletListAccountsParams, WalletListInstancesParams, WalletSetAccountLabelParams,
    WalletStatusParams,
};

use crate::{
    config::{LocalAccountDirBackendConfig, WalletBackendConfig},
    coordinator_runtime::CoordinatorHandle,
};

const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ServerPolicy {
    channel: Channel,
    wallet_backends: Vec<WalletBackendConfig>,
}

impl ServerPolicy {
    pub fn new(channel: Channel, wallet_backends: Vec<WalletBackendConfig>) -> Self {
        Self {
            channel,
            wallet_backends,
        }
    }

    fn accepts_extension(
        &self,
        extension_id: &str,
    ) -> Option<&crate::config::StarmaskExtensionBackendConfig> {
        self.wallet_backends
            .iter()
            .filter_map(WalletBackendConfig::as_extension)
            .find(|backend| backend.allowed_extension_ids().contains(extension_id))
    }

    fn configured_backend(
        &self,
        wallet_instance_id: &starmask_types::WalletInstanceId,
    ) -> Option<&WalletBackendConfig> {
        self.wallet_backends
            .iter()
            .find(|backend| backend.backend_id() == wallet_instance_id.as_str())
    }

    fn configured_local_backend(
        &self,
        wallet_instance_id: &starmask_types::WalletInstanceId,
    ) -> Option<&LocalAccountDirBackendConfig> {
        self.configured_backend(wallet_instance_id)
            .and_then(WalletBackendConfig::as_local_account_dir)
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
        "wallet.setAccountLabel" => {
            let params = decode_protocol::<WalletSetAccountLabelParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::WalletSetAccountLabel(
                        SetAccountLabelCommand {
                            wallet_instance_id: params.wallet_instance_id,
                            address: params.address,
                            label: params.label,
                        },
                    ))
                    .await,
                |response| match response {
                    CoordinatorResponse::WalletAccountLabelSet(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.createAccount" => {
            let params = decode_protocol::<CreateAccountParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::CreateAccount(CreateAccountCommand {
                        client_request_id: params.client_request_id,
                        wallet_instance_id: params.wallet_instance_id,
                        display_hint: params.display_hint,
                        client_context: params.client_context,
                        ttl_seconds: params.ttl_seconds,
                    }))
                    .await,
                |response| match response {
                    CoordinatorResponse::RequestCreated(result) => Ok(result),
                    other => Err(unexpected_response(other)),
                },
            )?)
            .map_err(|error| error_response(None, error))?
        }
        "request.createExportAccount" => {
            let params = decode_protocol::<CreateExportAccountParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::CreateExportAccount(
                        CreateExportAccountCommand {
                            client_request_id: params.client_request_id,
                            account_address: params.account_address,
                            wallet_instance_id: params.wallet_instance_id,
                            output_file: params.output_file,
                            force: params.force,
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
        "request.createImportAccount" => {
            let params = decode_protocol::<CreateImportAccountParams>(&id, &request.params)?;
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::CreateImportAccount(
                        CreateImportAccountCommand {
                            client_request_id: params.client_request_id,
                            account_address: params.account_address,
                            wallet_instance_id: params.wallet_instance_id,
                            private_key_file: params.private_key_file,
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
            let params = decode_protocol_one_of::<RequestHasAvailableParams>(
                &id,
                &request.params,
                &[DAEMON_PROTOCOL_VERSION, GENERIC_BACKEND_PROTOCOL_VERSION],
            )?;
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
            let Some(config) = policy.accepts_extension(&params.extension_id) else {
                warn!(
                    channel = ?policy.channel,
                    extension_id = %params.extension_id,
                    "rejected extension registration outside allowlist"
                );
                return Ok(JsonRpcResponse::Success(JsonRpcSuccess::new(
                    id,
                    serde_json::to_value(ExtensionRegisteredResult {
                        wallet_instance_id: params.wallet_instance_id,
                        daemon_protocol_version: DAEMON_PROTOCOL_VERSION,
                        accepted: false,
                    })
                    .map_err(|error| error_response(None, error))?,
                )));
            };
            let wallet_instance_id = params.wallet_instance_id.clone();
            let lock_state = params.lock_state;
            let extension_id = params.extension_id.clone();
            let extension_version = params.extension_version.clone();
            let profile_hint = params
                .profile_hint
                .clone()
                .or_else(|| config.profile_hint().map(ToOwned::to_owned));
            expect_response(
                handle
                    .dispatch(CoordinatorCommand::RegisterBackend(
                        RegisterBackendCommand {
                            wallet_instance_id: wallet_instance_id.clone(),
                            backend_kind: starmask_types::BackendKind::StarmaskExtension,
                            transport_kind: starmask_types::TransportKind::NativeMessaging,
                            approval_surface: starmask_types::ApprovalSurface::BrowserUi,
                            instance_label: params
                                .profile_hint
                                .clone()
                                .unwrap_or_else(|| config.instance_label().to_owned()),
                            extension_id: extension_id.clone(),
                            extension_version: extension_version.clone(),
                            protocol_version: params.protocol_version,
                            capabilities: vec![
                                WalletCapability::GetPublicKey,
                                WalletCapability::SignMessage,
                                WalletCapability::SignTransaction,
                            ],
                            backend_metadata: serde_json::json!({
                                "backend_id": config.backend_id(),
                                "extension_id": extension_id,
                                "extension_version": extension_version,
                                "native_host_name": config.native_host_name(),
                                "profile_hint": profile_hint,
                            }),
                            profile_hint,
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
                daemon_protocol_version: DAEMON_PROTOCOL_VERSION,
                accepted: true,
            })
            .map_err(|error| error_response(None, error))?
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
        "backend.register" => {
            let params = decode_protocol_exact::<BackendRegisterParams>(
                &id,
                &request.params,
                GENERIC_BACKEND_PROTOCOL_VERSION,
            )?;
            let config = policy
                .configured_local_backend(&params.wallet_instance_id)
                .ok_or_else(|| {
                    JsonRpcErrorResponse::new(
                        &id,
                        SharedError::new(
                            SharedErrorCode::BackendNotAllowed,
                            "wallet_instance_id is not configured as an enabled local backend",
                        )
                        .with_retryable(false),
                    )
                })?;
            validate_generic_backend_registration(config, &params)
                .map_err(|error| JsonRpcErrorResponse::new(&id, error))?;
            let wallet_instance_id = params.wallet_instance_id.clone();
            expect_response(
                handle
                    .dispatch(CoordinatorCommand::RegisterBackend(
                        RegisterBackendCommand {
                            wallet_instance_id: wallet_instance_id.clone(),
                            backend_kind: params.backend_kind,
                            transport_kind: params.transport_kind,
                            approval_surface: params.approval_surface,
                            instance_label: params.instance_label,
                            extension_id: String::new(),
                            extension_version: String::new(),
                            protocol_version: params.protocol_version,
                            capabilities: canonical_capabilities(params.capabilities),
                            backend_metadata: params.backend_metadata,
                            profile_hint: None,
                            lock_state: params.lock_state,
                            accounts: params
                                .accounts
                                .into_iter()
                                .map(|account| {
                                    backend_account_to_wallet_account(&wallet_instance_id, account)
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
            serde_json::to_value(BackendRegisteredResult {
                wallet_instance_id,
                daemon_protocol_version: GENERIC_BACKEND_PROTOCOL_VERSION,
                accepted: true,
            })
            .map_err(|error| error_response(None, error))?
        }
        "backend.heartbeat" => {
            let params = decode_protocol_exact::<BackendHeartbeatParams>(
                &id,
                &request.params,
                GENERIC_BACKEND_PROTOCOL_VERSION,
            )?;
            if policy
                .configured_local_backend(&params.wallet_instance_id)
                .is_none()
            {
                return Err(JsonRpcErrorResponse::new(
                    &id,
                    SharedError::new(
                        SharedErrorCode::BackendNotAllowed,
                        "wallet_instance_id is not configured as an enabled local backend",
                    )
                    .with_retryable(false),
                ));
            }
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::HeartbeatBackend(
                        HeartbeatBackendCommand {
                            wallet_instance_id: params.wallet_instance_id,
                            presented_request_ids: params.presented_request_ids,
                            lock_state: params.lock_state,
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
        "backend.updateAccounts" => {
            let params = decode_protocol_exact::<BackendUpdateAccountsParams>(
                &id,
                &request.params,
                GENERIC_BACKEND_PROTOCOL_VERSION,
            )?;
            let Some(config) = policy.configured_local_backend(&params.wallet_instance_id) else {
                return Err(JsonRpcErrorResponse::new(
                    &id,
                    SharedError::new(
                        SharedErrorCode::BackendNotAllowed,
                        "wallet_instance_id is not configured as an enabled local backend",
                    )
                    .with_retryable(false),
                ));
            };
            validate_local_backend_capabilities(config, &params.capabilities)
                .map_err(|error| JsonRpcErrorResponse::new(&id, error))?;
            let capabilities = canonical_capabilities(params.capabilities);
            serde_json::to_value(expect_response(
                handle
                    .dispatch(CoordinatorCommand::UpdateBackendAccounts(
                        UpdateBackendAccountsCommand {
                            wallet_instance_id: params.wallet_instance_id.clone(),
                            lock_state: params.lock_state,
                            capabilities,
                            accounts: params
                                .accounts
                                .into_iter()
                                .map(|account| {
                                    backend_account_to_wallet_account(
                                        &params.wallet_instance_id,
                                        account,
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
            let params = decode_protocol_one_of::<RequestPullNextParams>(
                &id,
                &request.params,
                &[DAEMON_PROTOCOL_VERSION, GENERIC_BACKEND_PROTOCOL_VERSION],
            )?;
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
            let params = decode_protocol_one_of::<RequestPresentedParams>(
                &id,
                &request.params,
                &[DAEMON_PROTOCOL_VERSION, GENERIC_BACKEND_PROTOCOL_VERSION],
            )?;
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
            let params = decode_protocol_one_of::<RequestResolveParams>(
                &id,
                &request.params,
                &[DAEMON_PROTOCOL_VERSION, GENERIC_BACKEND_PROTOCOL_VERSION],
            )?;
            let result = request_result_from_params(&params)
                .map_err(|error| JsonRpcErrorResponse::new(&id, error))?;
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
            let params = decode_protocol_one_of::<RequestRejectParams>(
                &id,
                &request.params,
                &[DAEMON_PROTOCOL_VERSION, GENERIC_BACKEND_PROTOCOL_VERSION],
            )?;
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
    decode_protocol_exact(id, value, DAEMON_PROTOCOL_VERSION)
}

fn decode_protocol_exact<T>(
    id: &str,
    value: &Value,
    expected: u32,
) -> Result<T, JsonRpcErrorResponse>
where
    T: DeserializeOwned + HasProtocolVersion,
{
    let params = serde_json::from_value::<T>(value.clone())
        .map_err(|error| error_response(Some(id), error))?;
    if params.protocol_version() != expected {
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

fn decode_protocol_one_of<T>(
    id: &str,
    value: &Value,
    expected_versions: &[u32],
) -> Result<T, JsonRpcErrorResponse>
where
    T: DeserializeOwned + HasProtocolVersion,
{
    let params = serde_json::from_value::<T>(value.clone())
        .map_err(|error| error_response(Some(id), error))?;
    if !expected_versions.contains(&params.protocol_version()) {
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
    BackendHeartbeatParams,
    BackendRegisterParams,
    BackendUpdateAccountsParams,
    CancelRequestParams,
    CreateAccountParams,
    CreateExportAccountParams,
    CreateImportAccountParams,
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
    WalletSetAccountLabelParams,
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

fn invalid_request_error(message: impl Into<String>) -> SharedError {
    SharedError::new(SharedErrorCode::InvalidRequest, message).with_retryable(false)
}

fn required_request_field<T>(
    value: Option<T>,
    message: &'static str,
) -> std::result::Result<T, SharedError> {
    value.ok_or_else(|| invalid_request_error(message))
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

fn request_result_from_params(
    params: &RequestResolveParams,
) -> std::result::Result<RequestResult, SharedError> {
    match params.result_kind {
        ResultKind::SignedTransaction => {
            let signed_txn_bcs_hex = required_request_field(
                params.signed_txn_bcs_hex.clone(),
                "signed_txn_bcs_hex is required for signed_transaction",
            )?;
            Ok(RequestResult::SignedTransaction { signed_txn_bcs_hex })
        }
        ResultKind::SignedMessage => {
            let signature = required_request_field(
                params.signature.clone(),
                "signature is required for signed_message",
            )?;
            Ok(RequestResult::SignedMessage { signature })
        }
        ResultKind::CreatedAccount => Ok(RequestResult::CreatedAccount {
            address: required_request_field(
                params.created_account_address.clone(),
                "created_account_address is required for created_account",
            )?,
            public_key: required_request_field(
                params.created_account_public_key.clone(),
                "created_account_public_key is required for created_account",
            )?,
            curve: required_request_field(
                params.created_account_curve,
                "created_account_curve is required for created_account",
            )?,
            is_default: required_request_field(
                params.created_account_is_default,
                "created_account_is_default is required for created_account",
            )?,
            is_locked: required_request_field(
                params.created_account_is_locked,
                "created_account_is_locked is required for created_account",
            )?,
        }),
        ResultKind::ExportedAccount => Ok(RequestResult::ExportedAccount {
            address: required_request_field(
                params.exported_account_address.clone(),
                "exported_account_address is required for exported_account",
            )?,
            output_file: required_request_field(
                params.exported_account_output_file.clone(),
                "exported_account_output_file is required for exported_account",
            )?,
        }),
        ResultKind::ImportedAccount => Ok(RequestResult::ImportedAccount {
            address: required_request_field(
                params.imported_account_address.clone(),
                "imported_account_address is required for imported_account",
            )?,
            public_key: required_request_field(
                params.imported_account_public_key.clone(),
                "imported_account_public_key is required for imported_account",
            )?,
            curve: required_request_field(
                params.imported_account_curve,
                "imported_account_curve is required for imported_account",
            )?,
            is_default: required_request_field(
                params.imported_account_is_default,
                "imported_account_is_default is required for imported_account",
            )?,
            is_locked: required_request_field(
                params.imported_account_is_locked,
                "imported_account_is_locked is required for imported_account",
            )?,
        }),
        ResultKind::None => Err(invalid_request_error(
            "result_kind none is not valid for request.resolve",
        )),
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
        is_read_only: false,
        is_locked: lock_state != starmask_types::LockState::Unlocked,
        last_seen_at: TimestampMs::from_millis(0),
    }
}

fn backend_account_to_wallet_account(
    wallet_instance_id: &starmask_types::WalletInstanceId,
    account: starmask_types::BackendAccount,
) -> WalletAccountRecord {
    WalletAccountRecord {
        wallet_instance_id: wallet_instance_id.clone(),
        address: account.address,
        label: account.label,
        public_key: account.public_key,
        is_default: account.is_default,
        is_read_only: account.is_read_only,
        is_locked: account.is_locked,
        last_seen_at: TimestampMs::from_millis(0),
    }
}

fn canonical_capabilities(mut capabilities: Vec<WalletCapability>) -> Vec<WalletCapability> {
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn validate_generic_backend_registration(
    config: &LocalAccountDirBackendConfig,
    params: &BackendRegisterParams,
) -> Result<(), SharedError> {
    if params.backend_kind != starmask_types::BackendKind::LocalAccountDir {
        return Err(SharedError::new(
            SharedErrorCode::InvalidBackendRegistration,
            "backend_kind does not match the configured backend",
        )
        .with_retryable(false));
    }
    if params.transport_kind != starmask_types::TransportKind::LocalSocket {
        return Err(SharedError::new(
            SharedErrorCode::InvalidBackendRegistration,
            "transport_kind must be local_socket for generic backend registration",
        )
        .with_retryable(false));
    }
    if params.approval_surface != config.approval_surface() {
        return Err(SharedError::new(
            SharedErrorCode::InvalidBackendRegistration,
            "approval_surface does not match the configured backend",
        )
        .with_retryable(false));
    }
    if params.instance_label.trim().is_empty() {
        return Err(SharedError::new(
            SharedErrorCode::InvalidBackendRegistration,
            "instance_label cannot be empty",
        )
        .with_retryable(false));
    }
    if !params.backend_metadata.is_object() {
        return Err(SharedError::new(
            SharedErrorCode::InvalidBackendRegistration,
            "backend_metadata must be a JSON object",
        )
        .with_retryable(false));
    }
    let metadata_len = serde_json::to_vec(&params.backend_metadata)
        .map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidBackendRegistration,
                error.to_string(),
            )
        })?
        .len();
    if metadata_len > 4 * 1024 {
        return Err(SharedError::new(
            SharedErrorCode::InvalidBackendRegistration,
            "backend_metadata exceeds the 4 KiB phase-2 limit",
        )
        .with_retryable(false));
    }

    validate_local_backend_capabilities(config, &params.capabilities)
}

fn validate_local_backend_capabilities(
    config: &LocalAccountDirBackendConfig,
    capabilities: &[WalletCapability],
) -> Result<(), SharedError> {
    let requested = canonical_capabilities(capabilities.to_vec());
    let allowed = WalletBackendConfig::LocalAccountDir(config.clone()).allowed_capabilities();
    for capability in requested {
        if !allowed.contains(&capability) {
            return Err(SharedError::new(
                SharedErrorCode::InvalidBackendRegistration,
                format!("unsupported capability advertised: {capability:?}"),
            )
            .with_retryable(false));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::{ServerPolicy, dispatch_request};
    use crate::{
        config::{StarmaskExtensionBackendConfig, WalletBackendConfig},
        coordinator_runtime::CoordinatorHandle,
    };
    use starmask_types::{
        Channel, DAEMON_PROTOCOL_VERSION, ExtensionRegisteredResult, JsonRpcRequest,
        JsonRpcResponse, JsonRpcSuccess, LockState, SharedErrorCode, WalletInstanceId,
    };

    fn test_policy(allowed_extension_ids: &[&str]) -> ServerPolicy {
        ServerPolicy::new(
            Channel::Development,
            vec![WalletBackendConfig::StarmaskExtension(
                StarmaskExtensionBackendConfig::new(
                    "browser-default",
                    "Browser Default",
                    starmask_types::ApprovalSurface::BrowserUi,
                    allowed_extension_ids
                        .iter()
                        .map(|extension_id| (*extension_id).to_owned())
                        .collect(),
                    "com.starcoin.test",
                    None,
                ),
            )],
        )
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
        assert_eq!(error.error.code, SharedErrorCode::InvalidRequest);
        assert_eq!(error.error.retryable, Some(false));
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
