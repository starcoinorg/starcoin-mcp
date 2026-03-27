#![forbid(unsafe_code)]

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use starcoin_account::{AccountManager, account_storage::AccountStorage};
use starcoin_config::RocksdbConfig;
use starcoin_types::genesis_config::ChainId;
use starmask_types::{
    BackendAccount, BackendHeartbeatParams, BackendRegisterParams, BackendUpdateAccountsParams,
    LockState, PresentationId, PulledRequest, RejectReasonCode, RequestId, RequestResolveParams,
    RequestResult, ResultKind, TransportKind, WalletCapability, WalletInstanceId,
};
use starmaskd::config::LocalAccountDirBackendConfig;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    client::{DaemonRpc, LocalDaemonClient, daemon_protocol_version},
    request_support::{
        RequestRejection, account_info_to_backend_account, fulfill_request, parse_account_address,
    },
    tty_prompt::prompt_for_request,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    lock_state: LockState,
    capabilities: Vec<WalletCapability>,
    accounts: Vec<BackendAccount>,
}

#[derive(Clone, Debug)]
struct HeartbeatState {
    lock_state: LockState,
    presented_request_ids: Vec<RequestId>,
}

pub struct LocalAccountAgent {
    client: LocalDaemonClient,
    manager: AccountManager,
    config: LocalAccountDirBackendConfig,
    wallet_instance_id: WalletInstanceId,
    heartbeat_interval: Duration,
    heartbeat_state: Arc<Mutex<HeartbeatState>>,
}

impl LocalAccountAgent {
    pub fn new(
        socket_path: PathBuf,
        heartbeat_interval: Duration,
        config: LocalAccountDirBackendConfig,
    ) -> Result<Self> {
        let storage =
            AccountStorage::create_from_path(config.account_dir(), RocksdbConfig::default())
                .context("failed to open local account storage")?;
        let manager = AccountManager::new(storage, ChainId::new(config.chain_id()))
            .context("failed to open local account manager")?;
        let wallet_instance_id = WalletInstanceId::new(config.backend_id().to_owned())
            .map_err(|error| anyhow!(error.to_string()))?;

        Ok(Self {
            client: LocalDaemonClient::new(socket_path),
            manager,
            config,
            wallet_instance_id,
            heartbeat_interval,
            heartbeat_state: Arc::new(Mutex::new(HeartbeatState {
                lock_state: LockState::Unknown,
                presented_request_ids: Vec::new(),
            })),
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let mut snapshot = self.snapshot()?;
        self.set_lock_state(snapshot.lock_state)?;

        let registered = self.client.backend_register(BackendRegisterParams {
            protocol_version: daemon_protocol_version(),
            wallet_instance_id: self.wallet_instance_id.clone(),
            backend_kind: starmask_types::BackendKind::LocalAccountDir,
            transport_kind: TransportKind::LocalSocket,
            approval_surface: self.config.approval_surface(),
            instance_label: self.config.instance_label().to_owned(),
            lock_state: snapshot.lock_state,
            capabilities: snapshot.capabilities.clone(),
            backend_metadata: json!({
                "account_provider_kind": "local",
                "prompt_mode": "tty_prompt",
            }),
            accounts: snapshot.accounts.clone(),
        })?;
        if !registered.accepted {
            bail!(
                "daemon rejected backend registration for {}",
                self.wallet_instance_id
            );
        }

        self.spawn_heartbeat_loop()?;
        info!(
            "local backend agent registered as {}",
            self.wallet_instance_id
        );

        loop {
            self.sync_snapshot(&mut snapshot)?;

            match self
                .client
                .request_pull_next(starmask_types::RequestPullNextParams {
                    protocol_version: daemon_protocol_version(),
                    wallet_instance_id: self.wallet_instance_id.clone(),
                }) {
                Ok(result) => {
                    if let Some(request) = result.request {
                        self.handle_request(request, &snapshot)?;
                        self.sync_snapshot(&mut snapshot)?;
                        continue;
                    }
                }
                Err(error) => warn!("request.pullNext failed: {}", error.message),
            }

            thread::sleep(self.heartbeat_interval);
        }
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let mut accounts: Vec<_> = self
            .manager
            .list_account_infos()
            .context("failed to list local accounts")?
            .into_iter()
            .filter(|account| self.config.allow_read_only_accounts() || !account.is_readonly)
            .map(account_info_to_backend_account)
            .collect();
        accounts.sort_by(|left, right| left.address.cmp(&right.address));

        let has_signable = accounts.iter().any(|account| !account.is_read_only);
        let any_unlocked_signable = accounts
            .iter()
            .any(|account| !account.is_read_only && !account.is_locked);
        let lock_state = if !has_signable {
            LockState::Unknown
        } else if any_unlocked_signable {
            LockState::Unlocked
        } else {
            LockState::Locked
        };

        let capabilities = if has_signable {
            vec![
                WalletCapability::Unlock,
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ]
        } else if accounts.iter().any(|account| account.public_key.is_some()) {
            vec![WalletCapability::GetPublicKey]
        } else {
            Vec::new()
        };

        Ok(Snapshot {
            lock_state,
            capabilities,
            accounts,
        })
    }

    fn handle_request(&mut self, request: PulledRequest, snapshot: &Snapshot) -> Result<()> {
        let account_address = parse_account_address(&request.account_address)?;
        let active_presentation_id = request.presentation_id.as_ref();
        let Some(account_info) = self
            .manager
            .account_info(account_address)
            .context("failed to read local account state")?
        else {
            self.reject_request(
                &request,
                active_presentation_id,
                RejectReasonCode::BackendUnavailable,
                Some("Requested account is no longer available".to_owned()),
            )?;
            return Ok(());
        };

        if account_info.is_readonly {
            self.reject_request(
                &request,
                active_presentation_id,
                RejectReasonCode::UnsupportedOperation,
                Some("Read-only accounts cannot sign".to_owned()),
            )?;
            return Ok(());
        }

        let presentation_id = request
            .presentation_id
            .clone()
            .unwrap_or_else(new_presentation_id);
        self.client
            .request_presented(starmask_types::RequestPresentedParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                delivery_lease_id: request.delivery_lease_id.clone(),
                presentation_id: presentation_id.clone(),
            })?;
        self.push_presented_request(request.request_id.clone())?;

        let result = (|| {
            let approval = prompt_for_request(&request, &account_info, &snapshot.capabilities)?;

            if approval.approved() {
                fulfill_request(
                    &self.manager,
                    Duration::from_secs(self.config.unlock_cache_ttl().as_secs()),
                    &request,
                    account_address,
                    &account_info,
                    &snapshot.capabilities,
                    approval.password(),
                )
            } else {
                Err(RequestRejection {
                    reason_code: RejectReasonCode::RequestRejected,
                    message: Some("User rejected the signing request".to_owned()),
                })
            }
        })();

        self.pop_presented_request(&request.request_id)?;

        match result {
            Ok(result) => self.resolve_request(&request, &presentation_id, result),
            Err(rejection) => self.reject_request(
                &request,
                Some(&presentation_id),
                rejection.reason_code,
                rejection.message,
            ),
        }
    }

    fn resolve_request(
        &self,
        request: &PulledRequest,
        presentation_id: &PresentationId,
        result: RequestResult,
    ) -> Result<()> {
        let params = match result {
            RequestResult::SignedTransaction { signed_txn_bcs_hex } => RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::SignedTransaction,
                signed_txn_bcs_hex: Some(signed_txn_bcs_hex),
                signature: None,
            },
            RequestResult::SignedMessage { signature } => RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::SignedMessage,
                signed_txn_bcs_hex: None,
                signature: Some(signature),
            },
        };
        self.client.request_resolve(params)?;
        Ok(())
    }

    fn reject_request(
        &self,
        request: &PulledRequest,
        presentation_id: Option<&PresentationId>,
        reason_code: RejectReasonCode,
        reason_message: Option<String>,
    ) -> Result<()> {
        self.client
            .request_reject(starmask_types::RequestRejectParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.cloned(),
                reason_code,
                reason_message,
            })?;
        Ok(())
    }

    fn sync_snapshot(&mut self, current: &mut Snapshot) -> Result<()> {
        let next = self.snapshot()?;
        self.set_lock_state(next.lock_state)?;
        if *current == next {
            return Ok(());
        }
        self.client
            .backend_update_accounts(BackendUpdateAccountsParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                lock_state: next.lock_state,
                capabilities: next.capabilities.clone(),
                accounts: next.accounts.clone(),
            })?;
        *current = next;
        Ok(())
    }

    fn set_lock_state(&self, lock_state: LockState) -> Result<()> {
        let mut state = self
            .heartbeat_state
            .lock()
            .map_err(|_| anyhow!("heartbeat state lock poisoned"))?;
        state.lock_state = lock_state;
        Ok(())
    }

    fn push_presented_request(&self, request_id: RequestId) -> Result<()> {
        let mut state = self
            .heartbeat_state
            .lock()
            .map_err(|_| anyhow!("heartbeat state lock poisoned"))?;
        if !state.presented_request_ids.contains(&request_id) {
            state.presented_request_ids.push(request_id);
        }
        Ok(())
    }

    fn pop_presented_request(&self, request_id: &RequestId) -> Result<()> {
        let mut state = self
            .heartbeat_state
            .lock()
            .map_err(|_| anyhow!("heartbeat state lock poisoned"))?;
        state.presented_request_ids.retain(|id| id != request_id);
        Ok(())
    }

    fn spawn_heartbeat_loop(&self) -> Result<()> {
        let client = self.client.clone();
        let wallet_instance_id = self.wallet_instance_id.clone();
        let heartbeat_interval = self.heartbeat_interval;
        let state = Arc::clone(&self.heartbeat_state);

        thread::Builder::new()
            .name("starmask-local-heartbeat".to_owned())
            .spawn(move || {
                loop {
                    thread::sleep(heartbeat_interval);
                    let snapshot = match state.lock() {
                        Ok(guard) => guard.clone(),
                        Err(_) => {
                            warn!("heartbeat state lock poisoned");
                            continue;
                        }
                    };
                    if let Err(error) = client.backend_heartbeat(BackendHeartbeatParams {
                        protocol_version: daemon_protocol_version(),
                        wallet_instance_id: wallet_instance_id.clone(),
                        presented_request_ids: snapshot.presented_request_ids,
                        lock_state: Some(snapshot.lock_state),
                    }) {
                        warn!("backend.heartbeat failed: {}", error.message);
                    }
                }
            })
            .context("failed to spawn heartbeat thread")?;
        Ok(())
    }
}

fn new_presentation_id() -> PresentationId {
    PresentationId::new(format!("presentation-{}", Uuid::now_v7()))
        .expect("generated presentation id should be valid")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use starcoin_account::{AccountManager, account_storage::AccountStorage};
    use starcoin_config::RocksdbConfig;
    use starcoin_types::genesis_config::ChainId;
    use tempfile::tempdir;

    use super::LocalAccountAgent;
    use starmask_types::{DurationSeconds, LockState, WalletCapability};
    use starmaskd::config::{LocalAccountDirBackendConfig, LocalPromptMode};

    #[test]
    fn snapshot_marks_locked_signable_accounts_as_unlock_capable() {
        let tempdir = tempdir().unwrap();
        {
            let storage =
                AccountStorage::create_from_path(tempdir.path(), RocksdbConfig::default()).unwrap();
            let manager = AccountManager::new(storage, ChainId::test()).unwrap();
            let account = manager.create_account("hello").unwrap();
            manager.lock_account(*account.address()).unwrap();
        }

        let config = LocalAccountDirBackendConfig::new(
            "local-main",
            "Local Main",
            starmask_types::ApprovalSurface::TtyPrompt,
            tempdir.path().to_path_buf(),
            LocalPromptMode::TtyPrompt,
            255,
            DurationSeconds::new(30),
            true,
            false,
        );
        let agent = LocalAccountAgent::new(
            tempdir.path().join("daemon.sock"),
            Duration::from_secs(1),
            config,
        )
        .unwrap();
        let snapshot = agent.snapshot().unwrap();

        assert_eq!(snapshot.lock_state, LockState::Locked);
        assert_eq!(
            snapshot.capabilities,
            vec![
                WalletCapability::Unlock,
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ]
        );
        assert_eq!(snapshot.accounts.len(), 1);
    }
}
