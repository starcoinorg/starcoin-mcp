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
use starcoin_account_api::AccountInfo;
use starcoin_config::RocksdbConfig;
use starcoin_types::genesis_config::ChainId;
use starmask_types::{
    BackendAccount, BackendHeartbeatParams, BackendRegisterParams, BackendUpdateAccountsParams,
    LockState, PresentationId, PulledRequest, RejectReasonCode, RequestId, RequestResolveParams,
    RequestResult, ResultKind, SharedError, TransportKind, WalletCapability, WalletInstanceId,
};
use starmaskd::config::{LocalAccountDirBackendConfig, LocalPromptMode};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    client::{DaemonRpc, LocalDaemonClient, daemon_protocol_version},
    request_support::{
        RequestRejection, account_info_to_backend_account, create_account, fulfill_request,
        import_account, parse_account_address,
    },
    tty_prompt::{ApprovalPrompt, TtyApprovalPrompt},
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
    client: Arc<dyn DaemonRpc>,
    prompt: Arc<dyn ApprovalPrompt>,
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
        validate_prompt_mode(&config)?;
        let storage =
            AccountStorage::create_from_path(config.account_dir(), RocksdbConfig::default())
                .context("failed to open local account storage")?;
        let manager = AccountManager::new(storage, ChainId::new(config.chain_id()))
            .context("failed to open local account manager")?;

        Self::from_parts(
            Arc::new(LocalDaemonClient::new(socket_path)),
            Arc::new(TtyApprovalPrompt),
            manager,
            config,
            heartbeat_interval,
        )
    }

    fn from_parts(
        client: Arc<dyn DaemonRpc>,
        prompt: Arc<dyn ApprovalPrompt>,
        manager: AccountManager,
        config: LocalAccountDirBackendConfig,
        heartbeat_interval: Duration,
    ) -> Result<Self> {
        validate_prompt_mode(&config)?;
        let wallet_instance_id = WalletInstanceId::new(config.backend_id().to_owned())
            .map_err(|error| anyhow!(error.to_string()))?;

        Ok(Self {
            client,
            prompt,
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
        let mut snapshot = self.register_backend()?;

        self.spawn_heartbeat_loop()?;
        info!(
            "local backend agent registered as {}",
            self.wallet_instance_id
        );

        loop {
            self.sync_snapshot(&mut snapshot)?;

            match self.pull_next_request() {
                Ok(Some(request)) => {
                    self.handle_request(request, &snapshot)?;
                    self.sync_snapshot(&mut snapshot)?;
                    continue;
                }
                Ok(None) => {}
                Err(error) => warn!("request.pullNext failed: {}", error.message),
            }

            thread::sleep(self.heartbeat_interval);
        }
    }

    fn register_backend(&mut self) -> Result<Snapshot> {
        let snapshot = self.snapshot()?;
        self.set_lock_state(snapshot.lock_state)?;

        let registered = self
            .client
            .backend_register(self.registration_params(&snapshot))?;
        if !registered.accepted {
            bail!(
                "daemon rejected backend registration for {}",
                self.wallet_instance_id
            );
        }

        Ok(snapshot)
    }

    fn registration_params(&self, snapshot: &Snapshot) -> BackendRegisterParams {
        BackendRegisterParams {
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
                "prompt_mode": prompt_mode_label(self.config.prompt_mode()),
            }),
            accounts: snapshot.accounts.clone(),
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
                WalletCapability::CreateAccount,
                WalletCapability::ExportAccount,
                WalletCapability::ImportAccount,
            ]
        } else if accounts.iter().any(|account| account.public_key.is_some()) {
            vec![
                WalletCapability::GetPublicKey,
                WalletCapability::CreateAccount,
                WalletCapability::ImportAccount,
            ]
        } else {
            vec![
                WalletCapability::CreateAccount,
                WalletCapability::ImportAccount,
            ]
        };

        Ok(Snapshot {
            lock_state,
            capabilities,
            accounts,
        })
    }

    fn pull_next_request(&self) -> std::result::Result<Option<PulledRequest>, SharedError> {
        self.client
            .request_pull_next(starmask_types::RequestPullNextParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
            })
            .map(|result| result.request)
    }

    fn handle_request(&mut self, request: PulledRequest, snapshot: &Snapshot) -> Result<()> {
        if request.kind == starmask_types::RequestKind::CreateAccount {
            return self.handle_create_account_request(request, snapshot);
        }
        if request.kind == starmask_types::RequestKind::ImportAccount {
            return self.handle_import_account_request(request, snapshot);
        }
        let account_address = parse_account_address(&request.account_address)?;
        let active_presentation_id = request.presentation_id.as_ref();
        let account_info = match self.load_signing_account_info(account_address) {
            Ok(account_info) => account_info,
            Err(rejection) => {
                self.reject_request(
                    &request,
                    active_presentation_id,
                    rejection.reason_code,
                    rejection.message,
                )?;
                return Ok(());
            }
        };

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
            let approval =
                self.prompt
                    .prompt_for_request(&request, &account_info, &snapshot.capabilities)?;

            if approval.approved() {
                let account_info = self.load_signing_account_info(account_address)?;

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

    fn handle_create_account_request(
        &mut self,
        request: PulledRequest,
        snapshot: &Snapshot,
    ) -> Result<()> {
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
            let approval = self
                .prompt
                .prompt_for_create_account(&request, &snapshot.capabilities)?;
            if !approval.approved() {
                return Err(RequestRejection {
                    reason_code: RejectReasonCode::RequestRejected,
                    message: Some("User rejected the account-creation request".to_owned()),
                });
            }
            let password = approval.password().ok_or_else(|| RequestRejection {
                reason_code: RejectReasonCode::BackendPolicyBlocked,
                message: Some("Account creation requires a password".to_owned()),
            })?;
            create_account(&self.manager, password)
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

    fn handle_import_account_request(
        &mut self,
        request: PulledRequest,
        snapshot: &Snapshot,
    ) -> Result<()> {
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
            let approval = self
                .prompt
                .prompt_for_import_account(&request, &snapshot.capabilities)?;
            if !approval.approved() {
                return Err(RequestRejection {
                    reason_code: RejectReasonCode::RequestRejected,
                    message: Some("User rejected the account-import request".to_owned()),
                });
            }
            let password = approval.password().ok_or_else(|| RequestRejection {
                reason_code: RejectReasonCode::BackendPolicyBlocked,
                message: Some("Account import requires a password".to_owned()),
            })?;
            import_account(&self.manager, &request, password)
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

    fn load_signing_account_info(
        &self,
        account_address: starcoin_types::account_address::AccountAddress,
    ) -> std::result::Result<AccountInfo, RequestRejection> {
        let Some(account_info) = self
            .manager
            .account_info(account_address)
            .map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!("Failed to read local account state: {error}")),
            })?
        else {
            return Err(RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some("Requested account is no longer available".to_owned()),
            });
        };

        if account_info.is_readonly {
            return Err(RequestRejection {
                reason_code: RejectReasonCode::UnsupportedOperation,
                message: Some("Read-only accounts cannot sign".to_owned()),
            });
        }

        Ok(account_info)
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
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
                exported_account_address: None,
                exported_account_output_file: None,
                imported_account_address: None,
                imported_account_public_key: None,
                imported_account_curve: None,
                imported_account_is_default: None,
                imported_account_is_locked: None,
            },
            RequestResult::SignedMessage { signature } => RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::SignedMessage,
                signed_txn_bcs_hex: None,
                signature: Some(signature),
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
                exported_account_address: None,
                exported_account_output_file: None,
                imported_account_address: None,
                imported_account_public_key: None,
                imported_account_curve: None,
                imported_account_is_default: None,
                imported_account_is_locked: None,
            },
            RequestResult::CreatedAccount {
                address,
                public_key,
                curve,
                is_default,
                is_locked,
            } => RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::CreatedAccount,
                signed_txn_bcs_hex: None,
                signature: None,
                created_account_address: Some(address),
                created_account_public_key: Some(public_key),
                created_account_curve: Some(curve),
                created_account_is_default: Some(is_default),
                created_account_is_locked: Some(is_locked),
                exported_account_address: None,
                exported_account_output_file: None,
                imported_account_address: None,
                imported_account_public_key: None,
                imported_account_curve: None,
                imported_account_is_default: None,
                imported_account_is_locked: None,
            },
            RequestResult::ExportedAccount {
                address,
                output_file,
            } => RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::ExportedAccount,
                signed_txn_bcs_hex: None,
                signature: None,
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
                exported_account_address: Some(address),
                exported_account_output_file: Some(output_file),
                imported_account_address: None,
                imported_account_public_key: None,
                imported_account_curve: None,
                imported_account_is_default: None,
                imported_account_is_locked: None,
            },
            RequestResult::ImportedAccount {
                address,
                public_key,
                curve,
                is_default,
                is_locked,
            } => RequestResolveParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: self.wallet_instance_id.clone(),
                request_id: request.request_id.clone(),
                presentation_id: presentation_id.clone(),
                result_kind: ResultKind::ImportedAccount,
                signed_txn_bcs_hex: None,
                signature: None,
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
                exported_account_address: None,
                exported_account_output_file: None,
                imported_account_address: Some(address),
                imported_account_public_key: Some(public_key),
                imported_account_curve: Some(curve),
                imported_account_is_default: Some(is_default),
                imported_account_is_locked: Some(is_locked),
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
        let client = Arc::clone(&self.client);
        let wallet_instance_id = self.wallet_instance_id.clone();
        let heartbeat_interval = self.heartbeat_interval;
        let state = Arc::clone(&self.heartbeat_state);

        thread::Builder::new()
            .name("starmask-local-heartbeat".to_owned())
            .spawn(move || {
                loop {
                    thread::sleep(heartbeat_interval);
                    if let Err(error) = send_heartbeat(client.as_ref(), &wallet_instance_id, &state)
                    {
                        warn!("backend.heartbeat failed: {error}");
                    }
                }
            })
            .context("failed to spawn heartbeat thread")?;
        Ok(())
    }
}

fn validate_prompt_mode(config: &LocalAccountDirBackendConfig) -> Result<()> {
    if config.prompt_mode().approval_surface() != config.approval_surface() {
        bail!("local-account-agent requires matching approval_surface and prompt_mode");
    }
    if config.prompt_mode() != LocalPromptMode::TtyPrompt {
        bail!("local-account-agent currently supports only prompt_mode = tty_prompt");
    }
    Ok(())
}

fn prompt_mode_label(prompt_mode: LocalPromptMode) -> &'static str {
    match prompt_mode {
        LocalPromptMode::TtyPrompt => "tty_prompt",
        LocalPromptMode::DesktopPrompt => "desktop_prompt",
    }
}

fn send_heartbeat(
    client: &dyn DaemonRpc,
    wallet_instance_id: &WalletInstanceId,
    state: &Arc<Mutex<HeartbeatState>>,
) -> Result<()> {
    let snapshot = state
        .lock()
        .map_err(|_| anyhow!("heartbeat state lock poisoned"))?
        .clone();
    client
        .backend_heartbeat(BackendHeartbeatParams {
            protocol_version: daemon_protocol_version(),
            wallet_instance_id: wallet_instance_id.clone(),
            presented_request_ids: snapshot.presented_request_ids,
            lock_state: Some(snapshot.lock_state),
        })
        .map(|_| ())
        .map_err(|error| anyhow!(error.message))
}

fn new_presentation_id() -> PresentationId {
    PresentationId::new(format!("presentation-{}", Uuid::now_v7()))
        .expect("generated presentation id should be valid")
}

#[cfg(all(test, unix))]
mod stack_tests;

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::TryFrom,
        path::PathBuf,
        str::FromStr,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use pretty_assertions::assert_eq;
    use serde::Serialize;
    use starcoin_account::{AccountManager, account_storage::AccountStorage};
    use starcoin_account_api::{AccountInfo, AccountPrivateKey};
    use starcoin_config::RocksdbConfig;
    use starcoin_crypto::ValidCryptoMaterialStringExt;
    use starcoin_types::{
        account_address::AccountAddress,
        genesis_config::ChainId,
        sign_message::SignedMessage,
        transaction::{RawUserTransaction, Script, SignedUserTransaction, TransactionPayload},
    };
    use tempfile::tempdir;

    use super::{LocalAccountAgent, Snapshot, send_heartbeat};
    use crate::{
        client::{DaemonRpc, daemon_protocol_version},
        request_support::RequestRejection,
        tty_prompt::{ApprovalPrompt, PromptApproval},
    };
    use starmask_types::{
        AckResult, BackendHeartbeatParams, BackendRegisterParams, BackendRegisteredResult,
        BackendUpdateAccountsParams, ClientRequestId, DeliveryLeaseId, LockState, MessageFormat,
        PayloadHash, PulledRequest, RejectReasonCode, RequestId, RequestKind,
        RequestPresentedParams, RequestPullNextParams, RequestPullNextResult, RequestRejectParams,
        RequestResolveParams, ResultKind, WalletCapability,
    };
    use starmaskd::config::{LocalAccountDirBackendConfig, LocalPromptMode};

    #[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
    struct FakeDaemonState {
        registered: Vec<BackendRegisterParams>,
        heartbeats: Vec<BackendHeartbeatParams>,
        updated_accounts: Vec<BackendUpdateAccountsParams>,
        presented: Vec<RequestPresentedParams>,
        resolved: Vec<RequestResolveParams>,
        rejected: Vec<RequestRejectParams>,
    }

    #[derive(Default)]
    struct FakeDaemonClient {
        state: Mutex<FakeDaemonState>,
        pull_results: Mutex<VecDeque<RequestPullNextResult>>,
    }

    impl FakeDaemonClient {
        fn snapshot(&self) -> FakeDaemonState {
            self.state.lock().unwrap().clone()
        }
    }

    impl DaemonRpc for FakeDaemonClient {
        fn backend_register(
            &self,
            params: BackendRegisterParams,
        ) -> Result<BackendRegisteredResult, starmask_types::SharedError> {
            self.state.lock().unwrap().registered.push(params.clone());
            Ok(BackendRegisteredResult {
                wallet_instance_id: params.wallet_instance_id,
                daemon_protocol_version: daemon_protocol_version(),
                accepted: true,
            })
        }

        fn backend_heartbeat(
            &self,
            params: BackendHeartbeatParams,
        ) -> Result<AckResult, starmask_types::SharedError> {
            self.state.lock().unwrap().heartbeats.push(params);
            Ok(AckResult { ok: true })
        }

        fn backend_update_accounts(
            &self,
            params: BackendUpdateAccountsParams,
        ) -> Result<AckResult, starmask_types::SharedError> {
            self.state.lock().unwrap().updated_accounts.push(params);
            Ok(AckResult { ok: true })
        }

        fn request_pull_next(
            &self,
            params: RequestPullNextParams,
        ) -> Result<RequestPullNextResult, starmask_types::SharedError> {
            Ok(self
                .pull_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(RequestPullNextResult {
                    wallet_instance_id: params.wallet_instance_id,
                    request: None,
                }))
        }

        fn request_presented(
            &self,
            params: RequestPresentedParams,
        ) -> Result<AckResult, starmask_types::SharedError> {
            self.state.lock().unwrap().presented.push(params);
            Ok(AckResult { ok: true })
        }

        fn request_resolve(
            &self,
            params: RequestResolveParams,
        ) -> Result<AckResult, starmask_types::SharedError> {
            self.state.lock().unwrap().resolved.push(params);
            Ok(AckResult { ok: true })
        }

        fn request_reject(
            &self,
            params: RequestRejectParams,
        ) -> Result<AckResult, starmask_types::SharedError> {
            self.state.lock().unwrap().rejected.push(params);
            Ok(AckResult { ok: true })
        }
    }

    struct StubPrompt {
        response: Mutex<std::result::Result<PromptApproval, RequestRejection>>,
    }

    impl StubPrompt {
        fn approve(password: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                response: Mutex::new(Ok(PromptApproval {
                    approved: true,
                    password: password.map(str::to_owned),
                })),
            })
        }

        fn reject() -> Arc<Self> {
            Arc::new(Self {
                response: Mutex::new(Ok(PromptApproval {
                    approved: false,
                    password: None,
                })),
            })
        }

        fn cancel_password() -> Arc<Self> {
            Arc::new(Self {
                response: Mutex::new(Err(RequestRejection {
                    reason_code: RejectReasonCode::WalletLocked,
                    message: Some("Password entry was cancelled".to_owned()),
                })),
            })
        }
    }

    impl ApprovalPrompt for StubPrompt {
        fn prompt_for_request(
            &self,
            _request: &PulledRequest,
            _account_info: &AccountInfo,
            _capabilities: &[WalletCapability],
        ) -> std::result::Result<PromptApproval, RequestRejection> {
            self.response.lock().unwrap().clone()
        }

        fn prompt_for_create_account(
            &self,
            _request: &PulledRequest,
            _capabilities: &[WalletCapability],
        ) -> std::result::Result<PromptApproval, RequestRejection> {
            self.response.lock().unwrap().clone()
        }

        fn prompt_for_import_account(
            &self,
            _request: &PulledRequest,
            _capabilities: &[WalletCapability],
        ) -> std::result::Result<PromptApproval, RequestRejection> {
            self.response.lock().unwrap().clone()
        }
    }

    struct TestHarness {
        _tempdir: tempfile::TempDir,
        agent: LocalAccountAgent,
        client: Arc<FakeDaemonClient>,
        account_address: AccountAddress,
    }

    impl TestHarness {
        fn new(locked: bool, prompt: Arc<StubPrompt>) -> Self {
            let tempdir = tempdir().unwrap();
            let storage =
                AccountStorage::create_from_path(tempdir.path(), RocksdbConfig::default()).unwrap();
            let manager = AccountManager::new(storage, ChainId::test()).unwrap();
            let account = manager.create_account("hello").unwrap();
            if locked {
                manager.lock_account(*account.address()).unwrap();
            } else {
                manager
                    .unlock_account(*account.address(), "hello", Duration::from_secs(60))
                    .unwrap();
            }

            let client = Arc::new(FakeDaemonClient::default());
            let agent = LocalAccountAgent::from_parts(
                client.clone(),
                prompt,
                manager,
                test_config(
                    tempdir.path().to_path_buf(),
                    starmask_types::ApprovalSurface::TtyPrompt,
                    LocalPromptMode::TtyPrompt,
                ),
                Duration::from_secs(1),
            )
            .unwrap();

            Self {
                _tempdir: tempdir,
                agent,
                client,
                account_address: *account.address(),
            }
        }

        fn snapshot(&self) -> Snapshot {
            self.agent.snapshot().unwrap()
        }

        fn sign_message_request(&self, message: &str, format: MessageFormat) -> PulledRequest {
            PulledRequest {
                request_id: RequestId::new("req-sign-message").unwrap(),
                client_request_id: ClientRequestId::new("client-sign-message").unwrap(),
                kind: RequestKind::SignMessage,
                account_address: self.account_address.to_string(),
                payload_hash: PayloadHash::new("payload-sign-message").unwrap(),
                display_hint: Some("Sign message".to_owned()),
                client_context: Some("phase2-test".to_owned()),
                resume_required: false,
                delivery_lease_id: Some(DeliveryLeaseId::new("lease-sign-message").unwrap()),
                lease_expires_at: None,
                presentation_id: None,
                presentation_expires_at: None,
                raw_txn_bcs_hex: None,
                message: Some(message.to_owned()),
                message_format: Some(format),
                output_file: None,
                force: false,
                private_key_file: None,
            }
        }

        fn sign_transaction_request(&self) -> PulledRequest {
            let raw_txn = RawUserTransaction::new_with_default_gas_token(
                self.account_address,
                7,
                TransactionPayload::Script(Script::new(vec![], vec![], vec![])),
                1_000,
                1,
                100_000,
                ChainId::test(),
            );
            let raw_txn_bcs_hex =
                format!("0x{}", hex::encode(bcs_ext::to_bytes(&raw_txn).unwrap()));

            PulledRequest {
                request_id: RequestId::new("req-sign-transaction").unwrap(),
                client_request_id: ClientRequestId::new("client-sign-transaction").unwrap(),
                kind: RequestKind::SignTransaction,
                account_address: self.account_address.to_string(),
                payload_hash: PayloadHash::new("payload-sign-transaction").unwrap(),
                display_hint: Some("Sign transaction".to_owned()),
                client_context: Some("phase2-test".to_owned()),
                resume_required: false,
                delivery_lease_id: Some(DeliveryLeaseId::new("lease-sign-transaction").unwrap()),
                lease_expires_at: None,
                presentation_id: None,
                presentation_expires_at: None,
                raw_txn_bcs_hex: Some(raw_txn_bcs_hex),
                message: None,
                message_format: None,
                output_file: None,
                force: false,
                private_key_file: None,
            }
        }

        fn create_account_request(&self) -> PulledRequest {
            PulledRequest {
                request_id: RequestId::new("req-create-account").unwrap(),
                client_request_id: ClientRequestId::new("client-create-account").unwrap(),
                kind: RequestKind::CreateAccount,
                account_address: String::new(),
                payload_hash: PayloadHash::new("payload-create-account").unwrap(),
                display_hint: Some("Create account".to_owned()),
                client_context: Some("phase2-test".to_owned()),
                resume_required: false,
                delivery_lease_id: Some(DeliveryLeaseId::new("lease-create-account").unwrap()),
                lease_expires_at: None,
                presentation_id: None,
                presentation_expires_at: None,
                raw_txn_bcs_hex: None,
                message: None,
                message_format: None,
                output_file: None,
                force: false,
                private_key_file: None,
            }
        }

        fn export_account_request(&self, output_file: PathBuf) -> PulledRequest {
            PulledRequest {
                request_id: RequestId::new("req-export-account").unwrap(),
                client_request_id: ClientRequestId::new("client-export-account").unwrap(),
                kind: RequestKind::ExportAccount,
                account_address: self.account_address.to_string(),
                payload_hash: PayloadHash::new("payload-export-account").unwrap(),
                display_hint: Some("Export account".to_owned()),
                client_context: Some("phase2-test".to_owned()),
                resume_required: false,
                delivery_lease_id: Some(DeliveryLeaseId::new("lease-export-account").unwrap()),
                lease_expires_at: None,
                presentation_id: None,
                presentation_expires_at: None,
                raw_txn_bcs_hex: None,
                message: None,
                message_format: None,
                output_file: Some(output_file.to_string_lossy().into_owned()),
                force: false,
                private_key_file: None,
            }
        }

        fn import_account_request(
            &self,
            private_key_file: PathBuf,
            account_address: &AccountAddress,
        ) -> PulledRequest {
            PulledRequest {
                request_id: RequestId::new("req-import-account").unwrap(),
                client_request_id: ClientRequestId::new("client-import-account").unwrap(),
                kind: RequestKind::ImportAccount,
                account_address: account_address.to_string(),
                payload_hash: PayloadHash::new("payload-import-account").unwrap(),
                display_hint: Some("Import account".to_owned()),
                client_context: Some("phase2-test".to_owned()),
                resume_required: false,
                delivery_lease_id: Some(DeliveryLeaseId::new("lease-import-account").unwrap()),
                lease_expires_at: None,
                presentation_id: None,
                presentation_expires_at: None,
                raw_txn_bcs_hex: None,
                message: None,
                message_format: None,
                output_file: None,
                force: false,
                private_key_file: Some(private_key_file.to_string_lossy().into_owned()),
            }
        }
    }

    fn test_config(
        account_dir: PathBuf,
        approval_surface: starmask_types::ApprovalSurface,
        prompt_mode: LocalPromptMode,
    ) -> LocalAccountDirBackendConfig {
        LocalAccountDirBackendConfig::new(
            "local-default",
            "Local Default Wallet",
            approval_surface,
            account_dir,
            prompt_mode,
            ChainId::test().id(),
            starmask_types::DurationSeconds::new(30),
            true,
            false,
        )
    }

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

        let agent = LocalAccountAgent::new(
            tempdir.path().join("daemon.sock"),
            Duration::from_secs(1),
            test_config(
                tempdir.path().to_path_buf(),
                starmask_types::ApprovalSurface::TtyPrompt,
                LocalPromptMode::TtyPrompt,
            ),
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
                WalletCapability::CreateAccount,
                WalletCapability::ExportAccount,
                WalletCapability::ImportAccount,
            ]
        );
        assert_eq!(snapshot.accounts.len(), 1);
    }

    #[test]
    fn local_account_agent_rejects_desktop_prompt_config() {
        let tempdir = tempdir().unwrap();
        let storage =
            AccountStorage::create_from_path(tempdir.path(), RocksdbConfig::default()).unwrap();
        let manager = AccountManager::new(storage, ChainId::test()).unwrap();
        manager.create_account("hello").unwrap();

        let result = LocalAccountAgent::from_parts(
            Arc::new(FakeDaemonClient::default()),
            StubPrompt::approve(None),
            manager,
            test_config(
                tempdir.path().to_path_buf(),
                starmask_types::ApprovalSurface::DesktopPrompt,
                LocalPromptMode::DesktopPrompt,
            ),
            Duration::from_secs(1),
        );
        let Err(error) = result else {
            panic!("expected desktop prompt config to be rejected");
        };

        assert!(error.to_string().contains("prompt_mode = tty_prompt"));
    }

    #[test]
    fn handle_request_resolves_signed_message_for_unlocked_account() {
        let mut harness = TestHarness::new(false, StubPrompt::approve(None));
        let request = harness.sign_message_request("hello", MessageFormat::Utf8);
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.presented.len(), 1);
        assert_eq!(state.resolved.len(), 1);
        assert!(state.rejected.is_empty());
        assert_eq!(state.resolved[0].result_kind, ResultKind::SignedMessage);
        let signature = state.resolved[0].signature.clone().unwrap();
        let signed_message = SignedMessage::from_str(&signature).unwrap();
        signed_message.check_signature().unwrap();
        signed_message.check_account(ChainId::test(), None).unwrap();
    }

    #[test]
    fn handle_request_resolves_created_account_for_local_backend() {
        let mut harness = TestHarness::new(false, StubPrompt::approve(Some("new-account")));
        let request = harness.create_account_request();
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.presented.len(), 1);
        assert_eq!(state.resolved.len(), 1);
        assert!(state.rejected.is_empty());
        assert_eq!(state.resolved[0].result_kind, ResultKind::CreatedAccount);
        assert!(state.resolved[0].created_account_address.is_some());
        assert!(state.resolved[0].created_account_public_key.is_some());
        assert_eq!(
            state.resolved[0].created_account_curve,
            Some(starmask_types::Curve::Ed25519)
        );
        assert_eq!(state.resolved[0].created_account_is_default, Some(false));
        assert_eq!(state.resolved[0].created_account_is_locked, Some(true));
    }

    #[test]
    fn handle_request_resolves_exported_account_to_local_file() {
        let mut harness = TestHarness::new(false, StubPrompt::approve(Some("hello")));
        let output_file = harness._tempdir.path().join("exports/account.key");
        let request = harness.export_account_request(output_file.clone());
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.presented.len(), 1);
        assert_eq!(state.resolved.len(), 1);
        assert!(state.rejected.is_empty());
        assert_eq!(state.resolved[0].result_kind, ResultKind::ExportedAccount);
        assert_eq!(
            state.resolved[0].exported_account_address.as_deref(),
            Some(harness.account_address.to_string().as_str())
        );
        assert_eq!(
            state.resolved[0].exported_account_output_file.as_deref(),
            Some(output_file.to_string_lossy().as_ref())
        );
        assert!(output_file.exists());
    }

    #[test]
    fn handle_request_resolves_imported_account_from_local_file() {
        let mut harness = TestHarness::new(false, StubPrompt::approve(Some("imported")));
        let source_dir = tempdir().unwrap();
        let source_storage =
            AccountStorage::create_from_path(source_dir.path(), RocksdbConfig::default()).unwrap();
        let source_manager = AccountManager::new(source_storage, ChainId::test()).unwrap();
        let source_account = source_manager.create_account("source").unwrap();
        let private_key_bytes = source_manager
            .export_account(*source_account.address(), "source")
            .unwrap();
        let private_key = AccountPrivateKey::try_from(private_key_bytes.as_slice()).unwrap();
        let private_key_file = harness._tempdir.path().join("import.key");
        std::fs::write(
            &private_key_file,
            private_key.to_encoded_string().unwrap() + "\n",
        )
        .unwrap();
        let request =
            harness.import_account_request(private_key_file.clone(), source_account.address());
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.presented.len(), 1);
        assert_eq!(state.resolved.len(), 1);
        assert!(state.rejected.is_empty());
        assert_eq!(state.resolved[0].result_kind, ResultKind::ImportedAccount);
        assert_eq!(
            state.resolved[0].imported_account_address.as_deref(),
            Some(source_account.address().to_string().as_str())
        );
        assert!(state.resolved[0].imported_account_public_key.is_some());
    }

    #[test]
    fn handle_request_resolves_signed_transaction_for_unlocked_account() {
        let mut harness = TestHarness::new(false, StubPrompt::approve(None));
        let request = harness.sign_transaction_request();
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.presented.len(), 1);
        assert_eq!(state.resolved.len(), 1);
        assert!(state.rejected.is_empty());
        assert_eq!(state.resolved[0].result_kind, ResultKind::SignedTransaction);
        let signed_txn_bcs_hex = state.resolved[0].signed_txn_bcs_hex.clone().unwrap();
        let signed_txn_bytes = hex::decode(signed_txn_bcs_hex.trim_start_matches("0x")).unwrap();
        let signed_txn: SignedUserTransaction = bcs_ext::from_bytes(&signed_txn_bytes).unwrap();
        assert_eq!(signed_txn.sender(), harness.account_address);
    }

    #[test]
    fn handle_request_unlocks_locked_account_before_signing() {
        let mut harness = TestHarness::new(true, StubPrompt::approve(Some("hello")));
        let request = harness.sign_message_request("hello", MessageFormat::Utf8);
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.resolved.len(), 1);
        assert!(state.rejected.is_empty());
    }

    #[test]
    fn handle_request_rejects_when_unlock_password_is_wrong() {
        let mut harness = TestHarness::new(true, StubPrompt::approve(Some("wrong")));
        let request = harness.sign_message_request("hello", MessageFormat::Utf8);
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert!(state.resolved.is_empty());
        assert_eq!(state.rejected.len(), 1);
        assert_eq!(
            state.rejected[0].reason_code,
            RejectReasonCode::WalletLocked
        );
        assert!(
            state.rejected[0]
                .reason_message
                .as_deref()
                .unwrap()
                .contains("Failed to unlock local account")
        );
    }

    #[test]
    fn handle_request_rejects_message_when_user_declines() {
        let mut harness = TestHarness::new(false, StubPrompt::reject());
        let request = harness.sign_message_request("hello", MessageFormat::Utf8);
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert!(state.resolved.is_empty());
        assert_eq!(state.rejected.len(), 1);
        assert_eq!(
            state.rejected[0].reason_code,
            RejectReasonCode::RequestRejected
        );
        assert_eq!(
            state.rejected[0].reason_message.as_deref(),
            Some("User rejected the signing request")
        );
    }

    #[test]
    fn handle_request_rejects_transaction_when_user_declines() {
        let mut harness = TestHarness::new(false, StubPrompt::reject());
        let request = harness.sign_transaction_request();
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert!(state.resolved.is_empty());
        assert_eq!(state.rejected.len(), 1);
        assert_eq!(
            state.rejected[0].reason_code,
            RejectReasonCode::RequestRejected
        );
        assert_eq!(
            state.rejected[0].reason_message.as_deref(),
            Some("User rejected the signing request")
        );
    }

    #[test]
    fn handle_request_rejects_when_unlock_is_cancelled() {
        let mut harness = TestHarness::new(true, StubPrompt::cancel_password());
        let request = harness.sign_message_request("hello", MessageFormat::Utf8);
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let state = harness.client.snapshot();
        assert!(state.resolved.is_empty());
        assert_eq!(state.rejected.len(), 1);
        assert_eq!(
            state.rejected[0].reason_code,
            RejectReasonCode::WalletLocked
        );
        assert_eq!(
            state.rejected[0].reason_message.as_deref(),
            Some("Password entry was cancelled")
        );
    }

    #[test]
    fn unlock_password_never_crosses_daemon_rpc_boundary() {
        let secret = "super-secret-password";
        let mut harness = TestHarness::new(true, StubPrompt::approve(Some(secret)));
        let request = harness.sign_message_request("approve this request", MessageFormat::Utf8);
        let snapshot = harness.snapshot();

        harness.agent.handle_request(request, &snapshot).unwrap();

        let transcript = serde_json::to_string(&harness.client.snapshot()).unwrap();
        assert!(!transcript.contains(secret));
    }

    #[test]
    fn read_only_accounts_are_listed_but_rejected_for_signing() {
        let account_dir = tempdir().unwrap();
        let storage =
            AccountStorage::create_from_path(account_dir.path(), RocksdbConfig::default()).unwrap();
        let manager = AccountManager::new(storage, ChainId::test()).unwrap();

        let source_dir = tempdir().unwrap();
        let source_storage =
            AccountStorage::create_from_path(source_dir.path(), RocksdbConfig::default()).unwrap();
        let source_manager = AccountManager::new(source_storage, ChainId::test()).unwrap();
        let source_account = source_manager.create_account("hello").unwrap();
        let source_info = source_manager
            .account_info(*source_account.address())
            .unwrap()
            .unwrap();
        manager
            .import_readonly_account(
                *source_account.address(),
                source_info.public_key.public_key_bytes().to_vec(),
            )
            .unwrap();

        let client = Arc::new(FakeDaemonClient::default());
        let mut agent = LocalAccountAgent::from_parts(
            client.clone(),
            StubPrompt::approve(None),
            manager,
            test_config(
                account_dir.path().to_path_buf(),
                starmask_types::ApprovalSurface::TtyPrompt,
                LocalPromptMode::TtyPrompt,
            ),
            Duration::from_secs(1),
        )
        .unwrap();
        let snapshot = agent.snapshot().unwrap();

        assert_eq!(snapshot.lock_state, LockState::Unknown);
        assert_eq!(
            snapshot.capabilities,
            vec![
                WalletCapability::GetPublicKey,
                WalletCapability::CreateAccount,
                WalletCapability::ImportAccount
            ]
        );
        assert_eq!(snapshot.accounts.len(), 1);
        assert!(snapshot.accounts[0].is_read_only);

        let request = PulledRequest {
            request_id: RequestId::new("req-read-only").unwrap(),
            client_request_id: ClientRequestId::new("client-read-only").unwrap(),
            kind: RequestKind::SignMessage,
            account_address: source_account.address().to_string(),
            payload_hash: PayloadHash::new("payload-read-only").unwrap(),
            display_hint: Some("Sign message".to_owned()),
            client_context: Some("phase2-test".to_owned()),
            resume_required: false,
            delivery_lease_id: Some(DeliveryLeaseId::new("lease-read-only").unwrap()),
            lease_expires_at: None,
            presentation_id: None,
            presentation_expires_at: None,
            raw_txn_bcs_hex: None,
            message: Some("hello".to_owned()),
            message_format: Some(MessageFormat::Utf8),
            output_file: None,
            force: false,
            private_key_file: None,
        };

        agent.handle_request(request, &snapshot).unwrap();

        let state = client.snapshot();
        assert!(state.resolved.is_empty());
        assert_eq!(state.rejected.len(), 1);
        assert_eq!(
            state.rejected[0].reason_code,
            RejectReasonCode::UnsupportedOperation
        );
        assert_eq!(
            state.rejected[0].reason_message.as_deref(),
            Some("Read-only accounts cannot sign")
        );
    }

    #[test]
    fn sync_snapshot_publishes_account_changes_once() {
        let mut harness = TestHarness::new(false, StubPrompt::approve(None));
        let mut current = harness.snapshot();
        let second = harness.agent.manager.create_account("second").unwrap();
        harness
            .agent
            .manager
            .unlock_account(*second.address(), "second", Duration::from_secs(60))
            .unwrap();

        harness.agent.sync_snapshot(&mut current).unwrap();
        harness.agent.sync_snapshot(&mut current).unwrap();

        let state = harness.client.snapshot();
        assert_eq!(state.updated_accounts.len(), 1);
        let mut expected_addresses = vec![
            harness.account_address.to_string(),
            second.address().to_string(),
        ];
        expected_addresses.sort();
        let mut actual_addresses = state.updated_accounts[0]
            .accounts
            .iter()
            .map(|account| account.address.clone())
            .collect::<Vec<_>>();
        actual_addresses.sort();
        assert_eq!(actual_addresses, expected_addresses);
    }

    #[test]
    fn send_heartbeat_reports_presented_requests_and_lock_state() {
        let harness = TestHarness::new(false, StubPrompt::approve(None));
        let request_id = RequestId::new("req-heartbeat").unwrap();
        harness
            .agent
            .push_presented_request(request_id.clone())
            .unwrap();
        harness.agent.set_lock_state(LockState::Locked).unwrap();

        send_heartbeat(
            harness.agent.client.as_ref(),
            &harness.agent.wallet_instance_id,
            &harness.agent.heartbeat_state,
        )
        .unwrap();

        let state = harness.client.snapshot();
        assert_eq!(
            state.heartbeats,
            vec![BackendHeartbeatParams {
                protocol_version: daemon_protocol_version(),
                wallet_instance_id: harness.agent.wallet_instance_id.clone(),
                presented_request_ids: vec![request_id],
                lock_state: Some(LockState::Locked),
            }]
        );
    }
}
