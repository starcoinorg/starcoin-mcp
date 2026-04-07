use thiserror::Error;

use starmask_types::{
    ClientRequestId, DeliveryLease, RequestId, RequestRecord, TimestampMs, WalletAccountRecord,
    WalletInstanceId, WalletInstanceRecord,
};

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("storage error: {0}")]
    Storage(String),
}

pub trait RequestRepository {
    fn get_request(
        &mut self,
        request_id: &RequestId,
    ) -> Result<Option<RequestRecord>, RepositoryError>;

    fn get_request_by_client_request_id(
        &mut self,
        client_request_id: &ClientRequestId,
    ) -> Result<Option<RequestRecord>, RepositoryError>;

    fn insert_request(&mut self, request: RequestRecord) -> Result<RequestRecord, RepositoryError>;

    fn update_request(&mut self, request: RequestRecord) -> Result<RequestRecord, RepositoryError>;

    fn claim_next_request_for_wallet(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        delivery_lease: DeliveryLease,
        now: TimestampMs,
    ) -> Result<Option<RequestRecord>, RepositoryError>;

    fn list_non_terminal_requests(&mut self) -> Result<Vec<RequestRecord>, RepositoryError>;

    fn list_terminal_requests_with_expired_results(
        &mut self,
        now: TimestampMs,
    ) -> Result<Vec<RequestRecord>, RepositoryError>;
}

pub trait WalletRepository {
    fn get_wallet_instance(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
    ) -> Result<Option<WalletInstanceRecord>, RepositoryError>;

    fn upsert_wallet_instance(
        &mut self,
        wallet_instance: WalletInstanceRecord,
    ) -> Result<(), RepositoryError>;

    fn list_wallet_instances(
        &mut self,
        connected_only: bool,
    ) -> Result<Vec<WalletInstanceRecord>, RepositoryError>;

    fn replace_wallet_accounts(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        accounts: Vec<WalletAccountRecord>,
    ) -> Result<(), RepositoryError>;

    fn list_wallet_accounts(
        &mut self,
        wallet_instance_id: Option<&WalletInstanceId>,
    ) -> Result<Vec<WalletAccountRecord>, RepositoryError>;

    fn get_wallet_account(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        address: &str,
    ) -> Result<Option<WalletAccountRecord>, RepositoryError>;
}

pub trait Store: RequestRepository + WalletRepository {}

impl<T> Store for T where T: RequestRepository + WalletRepository {}
