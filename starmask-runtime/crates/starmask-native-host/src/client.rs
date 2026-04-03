use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};

use starmask_types::{
    AckResult, DAEMON_PROTOCOL_VERSION, ExtensionHeartbeatParams, ExtensionRegisterParams,
    ExtensionRegisteredResult, ExtensionUpdateAccountsParams, GetRequestStatusParams,
    GetRequestStatusResult, JsonRpcRequest, JsonRpcResponse, JsonRpcSuccess,
    RequestHasAvailableParams, RequestHasAvailableResult, RequestPresentedParams,
    RequestPullNextParams, RequestPullNextResult, RequestRejectParams, RequestResolveParams,
    SharedError,
};

const RESPONSE_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

pub trait DaemonRpc {
    fn extension_register(
        &self,
        params: ExtensionRegisterParams,
    ) -> Result<ExtensionRegisteredResult, SharedError>;

    fn extension_heartbeat(
        &self,
        params: ExtensionHeartbeatParams,
    ) -> Result<AckResult, SharedError>;

    fn extension_update_accounts(
        &self,
        params: ExtensionUpdateAccountsParams,
    ) -> Result<AckResult, SharedError>;

    fn request_pull_next(
        &self,
        params: RequestPullNextParams,
    ) -> Result<RequestPullNextResult, SharedError>;

    fn request_has_available(
        &self,
        params: RequestHasAvailableParams,
    ) -> Result<RequestHasAvailableResult, SharedError>;

    fn get_request_status(
        &self,
        params: GetRequestStatusParams,
    ) -> Result<GetRequestStatusResult, SharedError>;

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
        let request = JsonRpcRequest::new("native-host", method, params);
        let encoded = serde_json::to_vec(&request).map_err(shared_internal_error)?;

        let mut stream = UnixStream::connect(&self.socket_path).map_err(shared_transport_error)?;
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
    fn extension_register(
        &self,
        params: ExtensionRegisterParams,
    ) -> Result<ExtensionRegisteredResult, SharedError> {
        self.call("extension.register", params)
    }

    fn extension_heartbeat(
        &self,
        params: ExtensionHeartbeatParams,
    ) -> Result<AckResult, SharedError> {
        self.call("extension.heartbeat", params)
    }

    fn extension_update_accounts(
        &self,
        params: ExtensionUpdateAccountsParams,
    ) -> Result<AckResult, SharedError> {
        self.call("extension.updateAccounts", params)
    }

    fn request_pull_next(
        &self,
        params: RequestPullNextParams,
    ) -> Result<RequestPullNextResult, SharedError> {
        self.call("request.pullNext", params)
    }

    fn request_has_available(
        &self,
        params: RequestHasAvailableParams,
    ) -> Result<RequestHasAvailableResult, SharedError> {
        self.call("request.hasAvailable", params)
    }

    fn get_request_status(
        &self,
        params: GetRequestStatusParams,
    ) -> Result<GetRequestStatusResult, SharedError> {
        self.call("request.getStatus", params)
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
    DAEMON_PROTOCOL_VERSION
}

fn shared_internal_error(error: impl std::fmt::Display) -> SharedError {
    starmask_types::SharedError::new(
        starmask_types::SharedErrorCode::InternalBridgeError,
        error.to_string(),
    )
}

fn shared_transport_error(error: impl std::fmt::Display) -> SharedError {
    starmask_types::SharedError::new(
        starmask_types::SharedErrorCode::RpcUnavailable,
        error.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
        sync::mpsc,
        thread,
    };

    use tempfile::tempdir;

    use super::*;
    use starmask_types::{
        JsonRpcErrorResponse, RequestId, RequestPullNextResult, RequestRejectParams,
        RequestResolveParams, SharedErrorCode, WalletInstanceId,
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
    fn request_pull_next_sends_expected_jsonrpc_request() {
        let response = serde_json::to_vec(&JsonRpcResponse::Success(JsonRpcSuccess::new(
            "native-host",
            RequestPullNextResult {
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                request: None,
            },
        )))
        .unwrap();
        let (_tempdir, socket_path, receiver, handle) = run_server_once(response);

        let client = LocalDaemonClient::new(socket_path);
        let result = client
            .request_pull_next(RequestPullNextParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
            })
            .unwrap();
        let request: JsonRpcRequest<serde_json::Value> =
            serde_json::from_slice(&receiver.recv().unwrap()).unwrap();

        assert_eq!(request.id, "native-host");
        assert_eq!(request.method, "request.pullNext");
        assert_eq!(
            request.params,
            serde_json::json!({
                "protocol_version": DAEMON_PROTOCOL_VERSION,
                "wallet_instance_id": "wallet-1",
            })
        );
        assert!(result.request.is_none());
        handle.join().unwrap();
    }

    #[test]
    fn request_reject_maps_daemon_error_response() {
        let response = serde_json::to_vec(&JsonRpcResponse::<AckResult>::Error(
            JsonRpcErrorResponse::new(
                "native-host",
                SharedError::new(SharedErrorCode::RequestNotFound, "request missing")
                    .with_retryable(false),
            ),
        ))
        .unwrap();
        let (_tempdir, socket_path, _receiver, handle) = run_server_once(response);

        let client = LocalDaemonClient::new(socket_path);
        let error = client
            .request_reject(RequestRejectParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                request_id: RequestId::new("request-1").unwrap(),
                presentation_id: None,
                reason_code: starmask_types::RejectReasonCode::RequestRejected,
                reason_message: None,
            })
            .unwrap_err();

        assert_eq!(error.code, SharedErrorCode::RequestNotFound);
        assert_eq!(error.message, "request missing");
        assert_eq!(error.retryable, Some(false));
        handle.join().unwrap();
    }

    #[test]
    fn oversized_daemon_response_is_reported_as_transport_error() {
        let response = vec![b'x'; usize::try_from(MAX_RESPONSE_BYTES).unwrap() + 1];
        let (_tempdir, socket_path, _receiver, handle) = run_server_once(response);

        let client = LocalDaemonClient::new(socket_path);
        let error = client
            .request_resolve(RequestResolveParams {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                wallet_instance_id: WalletInstanceId::new("wallet-1").unwrap(),
                request_id: RequestId::new("request-1").unwrap(),
                presentation_id: starmask_types::PresentationId::new("presentation-1").unwrap(),
                result_kind: starmask_types::ResultKind::SignedMessage,
                signed_txn_bcs_hex: None,
                signature: Some("0xsig".to_owned()),
            })
            .unwrap_err();

        assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
        assert!(
            error
                .message
                .contains("daemon response exceeded 1048576 bytes")
        );
        handle.join().unwrap();
    }
}
