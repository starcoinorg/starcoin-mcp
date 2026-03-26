#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use starmask_mcp::{
    AdapterError, DaemonClient, WalletListAccountsRequest, WalletListInstancesRequest,
};
use starmask_types::{
    CancelRequestResult, ClientRequestId, CreateRequestResult, CreateSignMessageParams,
    CreateSignTransactionParams, GetRequestStatusResult, LockState, MessageFormat, RequestId,
    RequestKind, RequestStatus, ResultKind, TimestampMs, WalletAccountGroup, WalletAccountSummary,
    WalletGetPublicKeyResult, WalletInstanceId, WalletInstanceSummary, WalletListAccountsResult,
    WalletListInstancesResult, WalletStatusResult,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FakeDaemonState {
    pub last_list_accounts: Option<WalletListAccountsRequest>,
    pub last_list_instances: Option<WalletListInstancesRequest>,
    pub last_get_public_key: Option<(String, Option<WalletInstanceId>)>,
    pub last_sign_transaction_request: Option<CreateSignTransactionParams>,
    pub last_sign_message_request: Option<CreateSignMessageParams>,
    pub last_get_request_status: Option<RequestId>,
    pub last_cancel_request: Option<RequestId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FakeDaemonResponses {
    pub wallet_status: Option<WalletStatusResult>,
    pub wallet_list_instances: Option<WalletListInstancesResult>,
    pub wallet_list_accounts: Option<WalletListAccountsResult>,
    pub wallet_get_public_key: Option<WalletGetPublicKeyResult>,
    pub create_sign_transaction_request: Option<CreateRequestResult>,
    pub create_sign_message_request: Option<CreateRequestResult>,
    pub get_request_status: Option<GetRequestStatusResult>,
    pub cancel_request: Option<CancelRequestResult>,
}

#[derive(Clone)]
pub struct FakeDaemonClient {
    state: Arc<Mutex<FakeDaemonState>>,
    responses: FakeDaemonResponses,
}

impl FakeDaemonClient {
    pub fn with_responses(responses: FakeDaemonResponses) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeDaemonState::default())),
            responses,
        }
    }

    pub fn state(&self) -> FakeDaemonState {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .clone()
    }
}

impl DaemonClient for FakeDaemonClient {
    async fn wallet_status(&self) -> Result<WalletStatusResult, AdapterError> {
        Ok(self
            .responses
            .wallet_status
            .clone()
            .expect("wallet_status response should be configured"))
    }

    async fn wallet_list_instances(
        &self,
        request: WalletListInstancesRequest,
    ) -> Result<WalletListInstancesResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_list_instances = Some(request);
        Ok(self
            .responses
            .wallet_list_instances
            .clone()
            .expect("wallet_list_instances response should be configured"))
    }

    async fn wallet_list_accounts(
        &self,
        request: WalletListAccountsRequest,
    ) -> Result<WalletListAccountsResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_list_accounts = Some(request);
        Ok(self
            .responses
            .wallet_list_accounts
            .clone()
            .expect("wallet_list_accounts response should be configured"))
    }

    async fn wallet_get_public_key(
        &self,
        address: String,
        wallet_instance_id: Option<WalletInstanceId>,
    ) -> Result<WalletGetPublicKeyResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_get_public_key = Some((address, wallet_instance_id));
        Ok(self
            .responses
            .wallet_get_public_key
            .clone()
            .expect("wallet_get_public_key response should be configured"))
    }

    async fn create_sign_transaction_request(
        &self,
        params: CreateSignTransactionParams,
    ) -> Result<CreateRequestResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_sign_transaction_request = Some(params);
        Ok(self
            .responses
            .create_sign_transaction_request
            .clone()
            .expect("create_sign_transaction_request response should be configured"))
    }

    async fn create_sign_message_request(
        &self,
        params: CreateSignMessageParams,
    ) -> Result<CreateRequestResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_sign_message_request = Some(params);
        Ok(self
            .responses
            .create_sign_message_request
            .clone()
            .expect("create_sign_message_request response should be configured"))
    }

    async fn get_request_status(
        &self,
        request_id: RequestId,
    ) -> Result<GetRequestStatusResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_get_request_status = Some(request_id);
        Ok(self
            .responses
            .get_request_status
            .clone()
            .expect("get_request_status response should be configured"))
    }

    async fn cancel_request(
        &self,
        request_id: RequestId,
    ) -> Result<CancelRequestResult, AdapterError> {
        self.state
            .lock()
            .expect("fake daemon state lock should succeed")
            .last_cancel_request = Some(request_id);
        Ok(self
            .responses
            .cancel_request
            .clone()
            .expect("cancel_request response should be configured"))
    }
}

pub fn wallet_instance_id() -> WalletInstanceId {
    WalletInstanceId::new("wallet-test-1").expect("wallet instance id should be valid")
}

pub fn sample_wallet_instance_summary(
    wallet_instance_id: &WalletInstanceId,
) -> WalletInstanceSummary {
    WalletInstanceSummary {
        wallet_instance_id: wallet_instance_id.clone(),
        extension_connected: true,
        lock_state: LockState::Unlocked,
        profile_hint: Some("primary".to_owned()),
        last_seen_at: TimestampMs::from_millis(1_710_000_000_000),
    }
}

pub fn sample_wallet_account_group(wallet_instance_id: &WalletInstanceId) -> WalletAccountGroup {
    WalletAccountGroup {
        wallet_instance_id: wallet_instance_id.clone(),
        extension_connected: true,
        lock_state: LockState::Unlocked,
        accounts: vec![WalletAccountSummary {
            address: "0x1".to_owned(),
            label: Some("Primary".to_owned()),
            public_key: Some("0xabc".to_owned()),
            is_default: true,
            is_locked: false,
        }],
    }
}

pub fn sample_create_request_result(kind: RequestKind) -> CreateRequestResult {
    CreateRequestResult {
        request_id: RequestId::new("request-1").expect("request id should be valid"),
        client_request_id: ClientRequestId::new("client-1")
            .expect("client request id should be valid"),
        kind,
        status: RequestStatus::Created,
        wallet_instance_id: wallet_instance_id(),
        created_at: TimestampMs::from_millis(1_710_000_000_000),
        expires_at: TimestampMs::from_millis(1_710_000_090_000),
    }
}

pub fn sample_wallet_status_result(wallet_instance_id: &WalletInstanceId) -> WalletStatusResult {
    WalletStatusResult {
        wallet_available: true,
        wallet_online: true,
        default_wallet_instance_id: Some(wallet_instance_id.clone()),
        wallet_instances: vec![sample_wallet_instance_summary(wallet_instance_id)],
    }
}

pub fn sample_message_status_result(
    wallet_instance_id: &WalletInstanceId,
) -> GetRequestStatusResult {
    GetRequestStatusResult {
        request_id: RequestId::new("request-1").expect("request id should be valid"),
        client_request_id: ClientRequestId::new("client-1")
            .expect("client request id should be valid"),
        kind: RequestKind::SignMessage,
        status: RequestStatus::Approved,
        wallet_instance_id: wallet_instance_id.clone(),
        created_at: TimestampMs::from_millis(1_710_000_000_000),
        expires_at: TimestampMs::from_millis(1_710_000_090_000),
        result_kind: ResultKind::SignedMessage,
        result_available: false,
        result_expires_at: None,
        error_code: None,
        error_message: None,
        result: None,
    }
}

pub fn sample_wallet_public_key_result(
    wallet_instance_id: &WalletInstanceId,
) -> WalletGetPublicKeyResult {
    WalletGetPublicKeyResult {
        wallet_instance_id: wallet_instance_id.clone(),
        address: "0x1".to_owned(),
        public_key: "0xabc".to_owned(),
        curve: starmask_types::Curve::Ed25519,
    }
}

pub fn sample_sign_message_params() -> CreateSignMessageParams {
    CreateSignMessageParams {
        protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
        client_request_id: ClientRequestId::new("client-1")
            .expect("client request id should be valid"),
        account_address: "0x1".to_owned(),
        wallet_instance_id: Some(wallet_instance_id()),
        message: "68656c6c6f".to_owned(),
        format: MessageFormat::Hex,
        display_hint: Some("Sign hello".to_owned()),
        client_context: Some("context".to_owned()),
        ttl_seconds: Some(starmask_types::DurationSeconds::new(90)),
    }
}
