use std::path::Path;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use tracing::{debug, warn};

use starmask_core::{
    CoordinatorCommand, CoordinatorResponse,
    commands::{CreateSignMessageCommand, CreateSignTransactionCommand},
};
use starmask_types::{
    CancelRequestParams, CreateSignMessageParams, CreateSignTransactionParams,
    GetRequestStatusParams, JsonRpcErrorResponse, JsonRpcRequest, JsonRpcResponse, JsonRpcSuccess,
    SharedError, SharedErrorCode, SystemGetInfoParams, SystemPingParams, WalletGetPublicKeyParams,
    WalletListAccountsParams, WalletListInstancesParams, WalletStatusParams,
};

use crate::coordinator_runtime::CoordinatorHandle;

pub async fn run_unix_server(socket_path: &Path, handle: CoordinatorHandle) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove {}", socket_path.display()))?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    loop {
        let (stream, _) = listener.accept().await?;
        let handle = handle.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, handle).await {
                warn!(%error, "failed to handle daemon rpc connection");
            }
        });
    }
}

async fn handle_connection(mut stream: UnixStream, handle: CoordinatorHandle) -> Result<()> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;
    if bytes.is_empty() {
        return Ok(());
    }

    let request: JsonRpcRequest<Value> = serde_json::from_slice(&bytes)?;
    debug!(method = %request.method, id = %request.id, "received daemon rpc request");
    let response = match dispatch_request(request, handle).await {
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
) -> Result<JsonRpcResponse<Value>, JsonRpcErrorResponse> {
    let id = request.id.clone();
    let result = match request.method.as_str() {
        "system.ping" => {
            decode_protocol::<SystemPingParams>(&request.params)?;
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
            decode_protocol::<SystemGetInfoParams>(&request.params)?;
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
            decode_protocol::<WalletStatusParams>(&request.params)?;
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
            let params = decode_protocol::<WalletListInstancesParams>(&request.params)?;
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
            let params = decode_protocol::<WalletListAccountsParams>(&request.params)?;
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
            let params = decode_protocol::<WalletGetPublicKeyParams>(&request.params)?;
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
            let params = decode_protocol::<CreateSignTransactionParams>(&request.params)?;
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
            let params = decode_protocol::<CreateSignMessageParams>(&request.params)?;
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
            let params = decode_protocol::<GetRequestStatusParams>(&request.params)?;
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
        "request.cancel" => {
            let params = decode_protocol::<CancelRequestParams>(&request.params)?;
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

fn decode_protocol<T>(value: &Value) -> Result<T, JsonRpcErrorResponse>
where
    T: DeserializeOwned + HasProtocolVersion,
{
    let params =
        serde_json::from_value::<T>(value.clone()).map_err(|error| error_response(None, error))?;
    if params.protocol_version() != starmask_types::DAEMON_PROTOCOL_VERSION {
        return Err(JsonRpcErrorResponse::new(
            "",
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
    GetRequestStatusParams,
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
