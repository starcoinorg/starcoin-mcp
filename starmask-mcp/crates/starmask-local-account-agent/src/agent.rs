use std::{
    io::{self, Write},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use starcoin_account::{AccountManager, account_storage::AccountStorage};
use starcoin_account_api::AccountInfo;
use starcoin_config::RocksdbConfig;
use starcoin_types::{
    account_address::AccountAddress, genesis_config::ChainId, sign_message::SigningMessage,
    transaction::RawUserTransaction,
};
use starmask_types::{
    BackendAccount, BackendHeartbeatParams, BackendRegisterParams, BackendUpdateAccountsParams,
    LockState, MessageFormat, PresentationId, PulledRequest, RejectReasonCode, RequestId,
    RequestKind, RequestResolveParams, RequestResult, ResultKind, TransportKind, WalletCapability,
    WalletInstanceId,
};
use starmaskd::config::LocalAccountDirBackendConfig;
use tracing::{info, warn};
use uuid::Uuid;

use crate::client::{DaemonRpc, LocalDaemonClient, daemon_protocol_version};

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

#[derive(Clone, Debug)]
struct PromptApproval {
    approved: bool,
    password: Option<String>,
}

#[derive(Clone, Debug)]
struct RequestRejection {
    reason_code: RejectReasonCode,
    message: Option<String>,
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
            AccountStorage::create_from_path(&config.account_dir, RocksdbConfig::default())
                .context("failed to open local account storage")?;
        let manager = AccountManager::new(storage, ChainId::new(config.chain_id))
            .context("failed to open local account manager")?;
        let wallet_instance_id = WalletInstanceId::new(config.common.backend_id.clone())
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
            approval_surface: self.config.common.approval_surface,
            instance_label: self.config.common.instance_label.clone(),
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
            .filter(|account| self.config.allow_read_only_accounts || !account.is_readonly)
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
        let account_address = AccountAddress::from_str(&request.account_address)
            .with_context(|| format!("invalid account address {}", request.account_address))?;
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
            let approval =
                self.prompt_for_request(&request, &account_info, &snapshot.capabilities)?;

            if approval.approved {
                self.fulfill_request(
                    &request,
                    account_address,
                    &account_info,
                    &snapshot.capabilities,
                    approval.password.as_deref(),
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

    fn fulfill_request(
        &self,
        request: &PulledRequest,
        account_address: AccountAddress,
        account_info: &AccountInfo,
        capabilities: &[WalletCapability],
        password: Option<&str>,
    ) -> std::result::Result<RequestResult, RequestRejection> {
        if account_info.is_locked {
            ensure_local_unlock_capability(account_info.is_locked, capabilities)?;
            let Some(password) = password else {
                return Err(RequestRejection {
                    reason_code: RejectReasonCode::WalletLocked,
                    message: Some("Local account is locked".to_owned()),
                });
            };
            self.manager
                .unlock_account(
                    account_address,
                    password,
                    Duration::from_secs(self.config.unlock_cache_ttl.as_secs()),
                )
                .map_err(|error| RequestRejection {
                    reason_code: RejectReasonCode::WalletLocked,
                    message: Some(format!("Failed to unlock local account: {error}")),
                })?;
        }

        match request.kind {
            RequestKind::SignTransaction => self.sign_transaction(request, account_address),
            RequestKind::SignMessage => self.sign_message(request, account_address),
        }
    }

    fn sign_transaction(
        &self,
        request: &PulledRequest,
        address: AccountAddress,
    ) -> std::result::Result<RequestResult, RequestRejection> {
        let raw_txn_hex = request
            .raw_txn_bcs_hex
            .as_deref()
            .ok_or_else(|| RequestRejection {
                reason_code: RejectReasonCode::InvalidTransactionPayload,
                message: Some("Missing raw transaction payload".to_owned()),
            })?;
        let raw_txn_bytes = decode_hex_bytes(raw_txn_hex).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::InvalidTransactionPayload,
            message: Some(error),
        })?;
        let raw_txn: RawUserTransaction =
            bcs_ext::from_bytes(&raw_txn_bytes).map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::InvalidTransactionPayload,
                message: Some(format!("Invalid raw transaction payload: {error}")),
            })?;
        if raw_txn.sender() != address {
            return Err(RequestRejection {
                reason_code: RejectReasonCode::InvalidTransactionPayload,
                message: Some("Raw transaction sender does not match request account".to_owned()),
            });
        }

        let signed_txn =
            self.manager
                .sign_txn(address, raw_txn)
                .map_err(|error| RequestRejection {
                    reason_code: RejectReasonCode::BackendUnavailable,
                    message: Some(format!("Failed to sign transaction: {error}")),
                })?;
        let signed_txn_bytes =
            bcs_ext::to_bytes(&signed_txn).map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!("Failed to serialize signed transaction: {error}")),
            })?;
        Ok(RequestResult::SignedTransaction {
            signed_txn_bcs_hex: format!("0x{}", hex::encode(signed_txn_bytes)),
        })
    }

    fn sign_message(
        &self,
        request: &PulledRequest,
        address: AccountAddress,
    ) -> std::result::Result<RequestResult, RequestRejection> {
        let message = request.message.as_deref().ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::InvalidMessagePayload,
            message: Some("Missing message payload".to_owned()),
        })?;
        let format = request.message_format.ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::InvalidMessagePayload,
            message: Some("Missing message format".to_owned()),
        })?;
        let signing_message =
            decode_signing_message(message, format).map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::InvalidMessagePayload,
                message: Some(error),
            })?;
        let signed_message = self
            .manager
            .sign_message(address, signing_message)
            .map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!("Failed to sign message: {error}")),
            })?;
        Ok(RequestResult::SignedMessage {
            signature: signed_message.to_string(),
        })
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

    fn prompt_for_request(
        &self,
        request: &PulledRequest,
        account_info: &AccountInfo,
        capabilities: &[WalletCapability],
    ) -> std::result::Result<PromptApproval, RequestRejection> {
        ensure_local_unlock_capability(account_info.is_locked, capabilities)?;
        print_request_summary(request, account_info);

        let approved =
            prompt_yes_no("Approve request? [y/N]: ").map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!("Failed to read local approval input: {error}")),
            })?;
        if !approved {
            return Ok(PromptApproval {
                approved: false,
                password: None,
            });
        }

        let password = if account_info.is_locked {
            let password = rpassword::prompt_password("Account password: ").map_err(|error| {
                RequestRejection {
                    reason_code: RejectReasonCode::BackendUnavailable,
                    message: Some(format!("Failed to read account password: {error}")),
                }
            })?;
            if password.is_empty() {
                return Err(RequestRejection {
                    reason_code: RejectReasonCode::WalletLocked,
                    message: Some("Password entry was cancelled".to_owned()),
                });
            }
            Some(password)
        } else {
            None
        };

        Ok(PromptApproval {
            approved: true,
            password,
        })
    }
}

fn account_info_to_backend_account(account: AccountInfo) -> BackendAccount {
    BackendAccount {
        address: account.address.to_string(),
        label: None,
        public_key: Some(format!(
            "0x{}",
            hex::encode(account.public_key.public_key_bytes())
        )),
        is_default: account.is_default,
        is_read_only: account.is_readonly,
        is_locked: account.is_locked,
    }
}

fn ensure_local_unlock_capability(
    account_locked: bool,
    capabilities: &[WalletCapability],
) -> std::result::Result<(), RequestRejection> {
    if account_locked && !capabilities.contains(&WalletCapability::Unlock) {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::WalletLocked,
            message: Some("Local account is locked".to_owned()),
        });
    }
    Ok(())
}

fn new_presentation_id() -> PresentationId {
    PresentationId::new(format!("presentation-{}", Uuid::now_v7()))
        .expect("generated presentation id should be valid")
}

fn decode_hex_bytes(input: &str) -> std::result::Result<Vec<u8>, String> {
    let trimmed = input.strip_prefix("0x").unwrap_or(input);
    hex::decode(trimmed).map_err(|error| format!("invalid hex payload: {error}"))
}

fn decode_signing_message(
    message: &str,
    format: MessageFormat,
) -> std::result::Result<SigningMessage, String> {
    match format {
        MessageFormat::Utf8 => Ok(SigningMessage::from(message.as_bytes().to_vec())),
        MessageFormat::Hex => decode_hex_bytes(message).map(SigningMessage::from),
    }
}

fn sanitize_for_tty(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '\n' => sanitized.push_str("\\n"),
            '\r' => sanitized.push_str("\\r"),
            '\t' => sanitized.push_str("\\t"),
            character if character.is_control() => {
                sanitized.push_str(&format!("\\u{{{:x}}}", u32::from(character)));
            }
            character => sanitized.push(character),
        }
    }
    sanitized
}

fn print_tty_field(label: &str, value: &str) {
    eprintln!("  {label}: {}", sanitize_for_tty(value));
}

fn print_untrusted_tty_field(label: &str, value: &str) {
    eprintln!("  {label} (untrusted): {}", sanitize_for_tty(value));
}

fn print_request_summary(request: &PulledRequest, account_info: &AccountInfo) {
    eprintln!();
    eprintln!("Starmask Local Signing Request");
    eprintln!("  Request ID: {}", request.request_id);
    let client_request_id = request.client_request_id.to_string();
    print_untrusted_tty_field("Client Request ID", &client_request_id);
    print_tty_field("Account", &request.account_address);
    eprintln!("  Account Locked: {}", account_info.is_locked);
    eprintln!("  Kind: {}", request_kind_label(request.kind));
    let payload_hash = request.payload_hash.to_string();
    print_tty_field("Payload Hash", &payload_hash);
    if let Some(display_hint) = &request.display_hint {
        print_untrusted_tty_field("Display Hint", display_hint);
    }
    if let Some(client_context) = &request.client_context {
        print_untrusted_tty_field("Client Context", client_context);
    }

    match request.kind {
        RequestKind::SignTransaction => {
            if let Some(raw_txn_bcs_hex) = &request.raw_txn_bcs_hex {
                print_untrusted_tty_field("Raw Transaction BCS", raw_txn_bcs_hex);
            }
        }
        RequestKind::SignMessage => {
            if let Some(message_format) = request.message_format {
                eprintln!("  Message Format: {}", message_format_label(message_format));
            }
            if let Some(message) = &request.message {
                print_untrusted_tty_field("Canonical Message", message);
            }
        }
    }
    eprintln!();
}

fn request_kind_label(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::SignTransaction => "sign_transaction",
        RequestKind::SignMessage => "sign_message",
    }
}

fn message_format_label(format: MessageFormat) -> &'static str {
    match format {
        MessageFormat::Utf8 => "utf8",
        MessageFormat::Hex => "hex",
    }
}

fn prompt_yes_no(prompt: &str) -> io::Result<bool> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(prompt.as_bytes())?;
    stdout.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let normalized = line.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use starcoin_account::{AccountManager, account_storage::AccountStorage};
    use starcoin_config::RocksdbConfig;
    use starcoin_types::genesis_config::ChainId;
    use tempfile::tempdir;

    use super::{
        LocalAccountAgent, account_info_to_backend_account, decode_signing_message,
        ensure_local_unlock_capability, sanitize_for_tty,
    };
    use starmask_types::{DurationSeconds, LockState, MessageFormat, WalletCapability};
    use starmaskd::config::{CommonBackendConfig, LocalAccountDirBackendConfig, LocalPromptMode};

    #[test]
    fn decode_signing_message_accepts_utf8_and_hex() {
        let utf8 = decode_signing_message("hello", MessageFormat::Utf8).unwrap();
        assert_eq!(utf8.to_string(), "0x68656c6c6f");

        let hex = decode_signing_message("0x010203", MessageFormat::Hex).unwrap();
        assert_eq!(hex.to_string(), "0x010203");
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

        let config = LocalAccountDirBackendConfig {
            common: CommonBackendConfig {
                backend_id: "local-main".to_owned(),
                instance_label: "Local Main".to_owned(),
                approval_surface: starmask_types::ApprovalSurface::TtyPrompt,
            },
            account_dir: tempdir.path().to_path_buf(),
            prompt_mode: LocalPromptMode::TtyPrompt,
            chain_id: 255,
            unlock_cache_ttl: DurationSeconds::new(30),
            allow_read_only_accounts: true,
            require_strict_permissions: false,
        };
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

    #[test]
    fn backend_account_uses_prefixed_public_key_hex() {
        let tempdir = tempdir().unwrap();
        let storage =
            AccountStorage::create_from_path(tempdir.path(), RocksdbConfig::default()).unwrap();
        let manager = AccountManager::new(storage, ChainId::test()).unwrap();
        let account = manager.create_account("hello").unwrap();
        let info = manager.account_info(*account.address()).unwrap().unwrap();
        let backend = account_info_to_backend_account(info);

        assert!(backend.public_key.unwrap().starts_with("0x"));
    }

    #[test]
    fn locked_accounts_fail_closed_without_unlock_capability() {
        let error = ensure_local_unlock_capability(
            true,
            &[
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
        )
        .unwrap_err();

        assert_eq!(
            error.reason_code,
            starmask_types::RejectReasonCode::WalletLocked
        );
        assert_eq!(error.message.as_deref(), Some("Local account is locked"));
    }

    #[test]
    fn unlocked_accounts_do_not_require_unlock_capability() {
        ensure_local_unlock_capability(
            false,
            &[
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
        )
        .unwrap();
    }

    #[test]
    fn sanitize_for_tty_escapes_control_sequences_but_preserves_unicode() {
        assert_eq!(
            sanitize_for_tty("hi\nthere\x1b[31m\t你好"),
            "hi\\nthere\\u{1b}[31m\\t你好"
        );
    }
}
