use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    thread,
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};

use starmask_types::{
    AckResult, BackendHeartbeatParams, BackendRegisterParams, BackendRegisteredResult,
    BackendUpdateAccountsParams, GENERIC_BACKEND_PROTOCOL_VERSION, JsonRpcRequest, JsonRpcResponse,
    JsonRpcSuccess, RequestPresentedParams, RequestPullNextParams, RequestPullNextResult,
    RequestRejectParams, RequestResolveParams, SharedError, SharedErrorCode,
};

const RESPONSE_READ_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(10);
const CONNECT_RETRY_ATTEMPTS: usize = 20;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

pub trait DaemonRpc: Send + Sync {
    fn backend_register(
        &self,
        params: BackendRegisterParams,
    ) -> Result<BackendRegisteredResult, SharedError>;

    fn backend_heartbeat(&self, params: BackendHeartbeatParams) -> Result<AckResult, SharedError>;

    fn backend_update_accounts(
        &self,
        params: BackendUpdateAccountsParams,
    ) -> Result<AckResult, SharedError>;

    fn request_pull_next(
        &self,
        params: RequestPullNextParams,
    ) -> Result<RequestPullNextResult, SharedError>;

    fn request_presented(&self, params: RequestPresentedParams) -> Result<AckResult, SharedError>;

    fn request_resolve(&self, params: RequestResolveParams) -> Result<AckResult, SharedError>;

    fn request_reject(&self, params: RequestRejectParams) -> Result<AckResult, SharedError>;
}

#[derive(Clone, Debug)]
pub struct LocalDaemonClient {
    socket_path: PathBuf,
}

impl LocalDaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    fn call<P, R>(&self, method: &str, params: P) -> Result<R, SharedError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let request = JsonRpcRequest::new("local-account-agent", method, params);
        let encoded = serde_json::to_vec(&request).map_err(shared_internal_error)?;

        let mut stream = connect_with_retry(&self.socket_path).map_err(shared_transport_error)?;
        stream
            .set_read_timeout(Some(RESPONSE_READ_TIMEOUT))
            .map_err(shared_transport_error)?;
        stream.write_all(&encoded).map_err(shared_transport_error)?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(shared_transport_error)?;

        let mut response = Vec::new();
        std::io::Read::by_ref(&mut stream)
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut response)
            .map_err(shared_transport_error)?;
        if u64::try_from(response.len()).unwrap_or(u64::MAX) > MAX_RESPONSE_BYTES {
            return Err(shared_transport_error(format!(
                "daemon response exceeded {MAX_RESPONSE_BYTES} bytes"
            )));
        }

        let response: JsonRpcResponse<R> =
            serde_json::from_slice(&response).map_err(shared_internal_error)?;
        match response {
            JsonRpcResponse::Success(JsonRpcSuccess { result, .. }) => Ok(result),
            JsonRpcResponse::Error(error) => Err(SharedError {
                code: error.error.code,
                message: error.error.message,
                retryable: error.error.retryable,
                details: error.error.details,
            }),
        }
    }
}

impl DaemonRpc for LocalDaemonClient {
    fn backend_register(
        &self,
        params: BackendRegisterParams,
    ) -> Result<BackendRegisteredResult, SharedError> {
        self.call("backend.register", params)
    }

    fn backend_heartbeat(&self, params: BackendHeartbeatParams) -> Result<AckResult, SharedError> {
        self.call("backend.heartbeat", params)
    }

    fn backend_update_accounts(
        &self,
        params: BackendUpdateAccountsParams,
    ) -> Result<AckResult, SharedError> {
        self.call("backend.updateAccounts", params)
    }

    fn request_pull_next(
        &self,
        params: RequestPullNextParams,
    ) -> Result<RequestPullNextResult, SharedError> {
        self.call("request.pullNext", params)
    }

    fn request_presented(&self, params: RequestPresentedParams) -> Result<AckResult, SharedError> {
        self.call("request.presented", params)
    }

    fn request_resolve(&self, params: RequestResolveParams) -> Result<AckResult, SharedError> {
        self.call("request.resolve", params)
    }

    fn request_reject(&self, params: RequestRejectParams) -> Result<AckResult, SharedError> {
        self.call("request.reject", params)
    }
}

pub fn daemon_protocol_version() -> u32 {
    GENERIC_BACKEND_PROTOCOL_VERSION
}

fn connect_with_retry(socket_path: &PathBuf) -> std::io::Result<UnixStream> {
    let mut last_error = None;
    for attempt in 0..CONNECT_RETRY_ATTEMPTS {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(error) if should_retry_connect(&error) && attempt + 1 < CONNECT_RETRY_ATTEMPTS => {
                last_error = Some(error);
                thread::sleep(CONNECT_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        std::io::Error::other("daemon socket connection retries exhausted")
    }))
}

fn should_retry_connect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::NotFound
    )
}

fn shared_internal_error(error: impl std::fmt::Display) -> SharedError {
    SharedError::new(SharedErrorCode::InternalBridgeError, error.to_string())
}

fn shared_transport_error(error: impl std::fmt::Display) -> SharedError {
    SharedError::new(SharedErrorCode::RpcUnavailable, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
        sync::mpsc,
        thread,
    };

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;
    use starmask_types::{
        BackendKind, BackendRegisteredResult, JsonRpcResponse, JsonRpcSuccess, LockState,
        TransportKind, WalletCapability, WalletInstanceId,
    };

    fn run_server_once(
        response: Vec<u8>,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        mpsc::Receiver<Vec<u8>>,
        thread::JoinHandle<()>,
    ) {
        let tempdir = tempdir().unwrap();
        let socket_path = tempdir.path().join("starmaskd.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let (sender, receiver) = mpsc::channel();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).unwrap();
            sender.send(request).unwrap();
            stream.write_all(&response).unwrap();
        });

        (tempdir, socket_path, receiver, handle)
    }

    #[test]
    fn backend_register_sends_expected_jsonrpc_request() {
        let response = serde_json::to_vec(&JsonRpcResponse::Success(JsonRpcSuccess::new(
            "local-account-agent",
            BackendRegisteredResult {
                wallet_instance_id: WalletInstanceId::new("local-main").unwrap(),
                daemon_protocol_version: GENERIC_BACKEND_PROTOCOL_VERSION,
                accepted: true,
            },
        )))
        .unwrap();
        let (_tempdir, socket_path, receiver, handle) = run_server_once(response);

        let client = LocalDaemonClient::new(socket_path);
        let result = client
            .backend_register(BackendRegisterParams {
                protocol_version: GENERIC_BACKEND_PROTOCOL_VERSION,
                wallet_instance_id: WalletInstanceId::new("local-main").unwrap(),
                backend_kind: BackendKind::LocalAccountDir,
                transport_kind: TransportKind::LocalSocket,
                approval_surface: starmask_types::ApprovalSurface::TtyPrompt,
                instance_label: "Local Main".to_owned(),
                lock_state: LockState::Locked,
                capabilities: vec![
                    WalletCapability::Unlock,
                    WalletCapability::GetPublicKey,
                    WalletCapability::SignMessage,
                    WalletCapability::SignTransaction,
                ],
                backend_metadata: serde_json::json!({"account_provider_kind": "local"}),
                accounts: Vec::new(),
            })
            .unwrap();
        let request: JsonRpcRequest<serde_json::Value> =
            serde_json::from_slice(&receiver.recv().unwrap()).unwrap();

        assert_eq!(request.id, "local-account-agent");
        assert_eq!(request.method, "backend.register");
        assert_eq!(
            request.params,
            serde_json::json!({
                "protocol_version": GENERIC_BACKEND_PROTOCOL_VERSION,
                "wallet_instance_id": "local-main",
                "backend_kind": "local_account_dir",
                "transport_kind": "local_socket",
                "approval_surface": "tty_prompt",
                "instance_label": "Local Main",
                "lock_state": "locked",
                "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
                "backend_metadata": {"account_provider_kind": "local"},
                "accounts": [],
            })
        );
        assert!(result.accepted);
        handle.join().unwrap();
    }
}
