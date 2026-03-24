use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};
use uuid::Uuid;

use starmask_types::{
    CancelRequestParams, CancelRequestResult, CreateRequestResult, CreateSignMessageParams,
    CreateSignTransactionParams, GetRequestStatusParams, GetRequestStatusResult, JsonRpcRequest,
    JsonRpcResponse, JsonRpcSuccess, SharedError, WalletGetPublicKeyParams,
    WalletGetPublicKeyResult, WalletListAccountsParams, WalletListAccountsResult,
    WalletListInstancesParams, WalletListInstancesResult, WalletStatusParams, WalletStatusResult,
};

use crate::error_mapping::AdapterError;

#[derive(Clone, Debug)]
pub struct LocalDaemonClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl LocalDaemonClient {
    pub fn new(socket_path: PathBuf, timeout: Duration) -> Self {
        Self {
            socket_path,
            timeout,
        }
    }

    pub async fn wallet_status(&self) -> Result<WalletStatusResult, AdapterError> {
        self.call(
            "wallet.status",
            WalletStatusParams {
                protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
            },
        )
        .await
    }

    pub async fn wallet_list_instances(
        &self,
        connected_only: bool,
    ) -> Result<WalletListInstancesResult, AdapterError> {
        self.call(
            "wallet.listInstances",
            WalletListInstancesParams {
                protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                connected_only,
            },
        )
        .await
    }

    pub async fn wallet_list_accounts(
        &self,
        wallet_instance_id: Option<starmask_types::WalletInstanceId>,
        include_public_key: bool,
    ) -> Result<WalletListAccountsResult, AdapterError> {
        self.call(
            "wallet.listAccounts",
            WalletListAccountsParams {
                protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                wallet_instance_id,
                include_public_key,
            },
        )
        .await
    }

    pub async fn wallet_get_public_key(
        &self,
        address: String,
        wallet_instance_id: Option<starmask_types::WalletInstanceId>,
    ) -> Result<WalletGetPublicKeyResult, AdapterError> {
        self.call(
            "wallet.getPublicKey",
            WalletGetPublicKeyParams {
                protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                address,
                wallet_instance_id,
            },
        )
        .await
    }

    pub async fn create_sign_transaction_request(
        &self,
        params: CreateSignTransactionParams,
    ) -> Result<CreateRequestResult, AdapterError> {
        self.call("request.createSignTransaction", params).await
    }

    pub async fn create_sign_message_request(
        &self,
        params: CreateSignMessageParams,
    ) -> Result<CreateRequestResult, AdapterError> {
        self.call("request.createSignMessage", params).await
    }

    pub async fn get_request_status(
        &self,
        request_id: starmask_types::RequestId,
    ) -> Result<GetRequestStatusResult, AdapterError> {
        self.call(
            "request.getStatus",
            GetRequestStatusParams {
                protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                request_id,
            },
        )
        .await
    }

    pub async fn cancel_request(
        &self,
        request_id: starmask_types::RequestId,
    ) -> Result<CancelRequestResult, AdapterError> {
        self.call(
            "request.cancel",
            CancelRequestParams {
                protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                request_id,
            },
        )
        .await
    }

    async fn call<P, R>(&self, method: &str, params: P) -> Result<R, AdapterError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let request = JsonRpcRequest::new(next_rpc_id(), method.to_owned(), params);
        let encoded = serde_json::to_vec(&request)?;

        let response_bytes = timeout(self.timeout, async {
            let mut stream = UnixStream::connect(&self.socket_path)
                .await
                .map_err(|error| AdapterError::Transport(error.to_string()))?;
            stream
                .write_all(&encoded)
                .await
                .map_err(|error| AdapterError::Transport(error.to_string()))?;
            stream
                .shutdown()
                .await
                .map_err(|error| AdapterError::Transport(error.to_string()))?;
            let mut response = Vec::new();
            stream
                .read_to_end(&mut response)
                .await
                .map_err(|error| AdapterError::Transport(error.to_string()))?;
            Ok::<Vec<u8>, AdapterError>(response)
        })
        .await
        .map_err(|error| AdapterError::Transport(error.to_string()))??;

        let response: JsonRpcResponse<R> = serde_json::from_slice(&response_bytes)?;
        match response {
            JsonRpcResponse::Success(JsonRpcSuccess { result, .. }) => Ok(result),
            JsonRpcResponse::Error(error) => Err(AdapterError::Shared(SharedError {
                code: error.error.code,
                message: error.error.message,
                retryable: error.error.retryable,
                details: error.error.details,
            })),
        }
    }
}

fn next_rpc_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("rpc-{millis}-{}", Uuid::now_v7())
}
