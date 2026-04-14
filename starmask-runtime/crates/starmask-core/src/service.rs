use std::collections::BTreeMap;

use tracing::debug;

use starmask_types::{
    ApprovalSurface, BackendKind, CancelRequestResult, ClientRequestId, CreateAccountPayload,
    CreateRequestResult, Curve, DAEMON_PROTOCOL_VERSION, DeliveryLease, DurationSeconds,
    GetRequestStatusResult, LockState, PayloadHash, PresentationLease, PulledRequest,
    RejectReasonCode, RequestHasAvailableResult, RequestId, RequestKind, RequestPayload,
    RequestPullNextResult as DaemonPullNextRequestResult, RequestRecord, RequestResult,
    RequestStatus, ResultKind, SharedErrorCode, SystemGetInfoResult, SystemPingResult, TimestampMs,
    TransactionPayload, TransportKind, WalletAccountGroup, WalletCapability,
    WalletGetPublicKeyResult, WalletInstanceId, WalletInstanceRecord, WalletListAccountsResult,
    WalletListInstancesResult, WalletStatusResult,
};

use crate::{
    commands::{
        CoordinatorCommand, CreateAccountCommand, CreateSignMessageCommand,
        CreateSignTransactionCommand, HeartbeatBackendCommand, HeartbeatExtensionCommand,
        MarkRequestPresentedCommand, RegisterBackendCommand, RegisterExtensionCommand,
        RejectRequestCommand, ResolveRequestCommand, UpdateBackendAccountsCommand,
        UpdateExtensionAccountsCommand,
    },
    error::{CoreError, CoreResult},
    policy::PolicyEngine,
    repo::Store,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorConfig {
    pub daemon_version: String,
    pub socket_scope: String,
    pub db_schema_version: u32,
    pub default_request_ttl: DurationSeconds,
    pub min_request_ttl: DurationSeconds,
    pub max_request_ttl: DurationSeconds,
    pub delivery_lease_ttl: DurationSeconds,
    pub presentation_lease_ttl: DurationSeconds,
    pub wallet_offline_after: DurationSeconds,
    pub result_retention: DurationSeconds,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            socket_scope: "local-user".to_owned(),
            db_schema_version: 2,
            default_request_ttl: DurationSeconds::new(300),
            min_request_ttl: DurationSeconds::new(30),
            max_request_ttl: DurationSeconds::new(3600),
            delivery_lease_ttl: DurationSeconds::new(30),
            presentation_lease_ttl: DurationSeconds::new(45),
            wallet_offline_after: DurationSeconds::new(25),
            result_retention: DurationSeconds::new(600),
        }
    }
}

pub trait Clock {
    fn now(&self) -> TimestampMs;
}

pub trait IdGenerator {
    fn new_request_id(&mut self) -> CoreResult<RequestId>;
    fn new_delivery_lease_id(&mut self) -> CoreResult<starmask_types::DeliveryLeaseId>;
}

pub type PullNextRequestResult = DaemonPullNextRequestResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestPresentedResult {
    pub request_id: RequestId,
    pub status: RequestStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestResolvedResult {
    pub request_id: RequestId,
    pub status: RequestStatus,
    pub result_kind: ResultKind,
    pub result_expires_at: Option<TimestampMs>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRejectedResult {
    pub request_id: RequestId,
    pub status: RequestStatus,
    pub error_code: SharedErrorCode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TickMaintenanceResult {
    pub expired_requests: usize,
    pub released_delivery_leases: usize,
    pub evicted_results: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoordinatorResponse {
    SystemPing(SystemPingResult),
    SystemInfo(SystemGetInfoResult),
    WalletStatus(WalletStatusResult),
    WalletInstances(WalletListInstancesResult),
    WalletAccounts(WalletListAccountsResult),
    WalletPublicKey(WalletGetPublicKeyResult),
    RequestCreated(CreateRequestResult),
    RequestStatus(GetRequestStatusResult),
    RequestHasAvailable(RequestHasAvailableResult),
    RequestCancelled(CancelRequestResult),
    PullNextRequest(PullNextRequestResult),
    RequestPresented(RequestPresentedResult),
    RequestResolved(RequestResolvedResult),
    RequestRejected(RequestRejectedResult),
    TickMaintenance(TickMaintenanceResult),
    Ack,
}

pub struct Coordinator<S, P, C, G> {
    store: S,
    policy: P,
    clock: C,
    id_generator: G,
    config: CoordinatorConfig,
}

impl<S, P, C, G> Coordinator<S, P, C, G>
where
    S: Store,
    P: PolicyEngine,
    C: Clock,
    G: IdGenerator,
{
    pub fn new(store: S, policy: P, clock: C, id_generator: G, config: CoordinatorConfig) -> Self {
        Self {
            store,
            policy,
            clock,
            id_generator,
            config,
        }
    }

    pub fn dispatch(&mut self, command: CoordinatorCommand) -> CoreResult<CoordinatorResponse> {
        match command {
            CoordinatorCommand::SystemPing => {
                Ok(CoordinatorResponse::SystemPing(self.system_ping()))
            }
            CoordinatorCommand::SystemGetInfo => {
                Ok(CoordinatorResponse::SystemInfo(self.system_get_info()))
            }
            CoordinatorCommand::WalletStatus => {
                Ok(CoordinatorResponse::WalletStatus(self.wallet_status()?))
            }
            CoordinatorCommand::WalletListInstances { connected_only } => Ok(
                CoordinatorResponse::WalletInstances(self.wallet_list_instances(connected_only)?),
            ),
            CoordinatorCommand::WalletListAccounts {
                wallet_instance_id,
                include_public_key,
            } => Ok(CoordinatorResponse::WalletAccounts(
                self.wallet_list_accounts(wallet_instance_id.as_ref(), include_public_key)?,
            )),
            CoordinatorCommand::WalletGetPublicKey {
                address,
                wallet_instance_id,
            } => Ok(CoordinatorResponse::WalletPublicKey(
                self.wallet_get_public_key(&address, wallet_instance_id.as_ref())?,
            )),
            CoordinatorCommand::CreateAccount(command) => Ok(CoordinatorResponse::RequestCreated(
                self.create_account_request(command)?,
            )),
            CoordinatorCommand::CreateSignTransaction(command) => Ok(
                CoordinatorResponse::RequestCreated(self.create_sign_transaction_request(command)?),
            ),
            CoordinatorCommand::CreateSignMessage(command) => Ok(
                CoordinatorResponse::RequestCreated(self.create_sign_message_request(command)?),
            ),
            CoordinatorCommand::GetRequestStatus { request_id } => Ok(
                CoordinatorResponse::RequestStatus(self.get_request_status(&request_id)?),
            ),
            CoordinatorCommand::RequestHasAvailable { wallet_instance_id } => {
                Ok(CoordinatorResponse::RequestHasAvailable(
                    self.request_has_available(&wallet_instance_id)?,
                ))
            }
            CoordinatorCommand::CancelRequest { request_id } => Ok(
                CoordinatorResponse::RequestCancelled(self.cancel_request(&request_id)?),
            ),
            CoordinatorCommand::RegisterExtension(command) => {
                self.register_extension(command)?;
                Ok(CoordinatorResponse::Ack)
            }
            CoordinatorCommand::RegisterBackend(command) => {
                self.register_backend(command)?;
                Ok(CoordinatorResponse::Ack)
            }
            CoordinatorCommand::HeartbeatExtension(command) => {
                self.heartbeat_extension(command)?;
                Ok(CoordinatorResponse::Ack)
            }
            CoordinatorCommand::HeartbeatBackend(command) => {
                self.heartbeat_backend(command)?;
                Ok(CoordinatorResponse::Ack)
            }
            CoordinatorCommand::UpdateExtensionAccounts(command) => {
                self.update_extension_accounts(command)?;
                Ok(CoordinatorResponse::Ack)
            }
            CoordinatorCommand::UpdateBackendAccounts(command) => {
                self.update_backend_accounts(command)?;
                Ok(CoordinatorResponse::Ack)
            }
            CoordinatorCommand::PullNextRequest { wallet_instance_id } => Ok(
                CoordinatorResponse::PullNextRequest(self.pull_next_request(&wallet_instance_id)?),
            ),
            CoordinatorCommand::MarkRequestPresented(command) => Ok(
                CoordinatorResponse::RequestPresented(self.mark_request_presented(command)?),
            ),
            CoordinatorCommand::ResolveRequest(command) => Ok(
                CoordinatorResponse::RequestResolved(self.resolve_request(command)?),
            ),
            CoordinatorCommand::RejectRequest(command) => Ok(CoordinatorResponse::RequestRejected(
                self.reject_request(command)?,
            )),
            CoordinatorCommand::TickMaintenance => Ok(CoordinatorResponse::TickMaintenance(
                self.tick_maintenance()?,
            )),
        }
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    fn system_ping(&self) -> SystemPingResult {
        SystemPingResult {
            ok: true,
            daemon_protocol_version: DAEMON_PROTOCOL_VERSION,
            daemon_version: self.config.daemon_version.clone(),
        }
    }

    fn system_get_info(&self) -> SystemGetInfoResult {
        SystemGetInfoResult {
            daemon_protocol_version: DAEMON_PROTOCOL_VERSION,
            daemon_version: self.config.daemon_version.clone(),
            socket_scope: self.config.socket_scope.clone(),
            db_schema_version: self.config.db_schema_version,
            result_retention_seconds: self.config.result_retention.as_secs(),
            default_request_ttl_seconds: self.config.default_request_ttl.as_secs(),
        }
    }

    fn wallet_status(&mut self) -> CoreResult<WalletStatusResult> {
        let instances = self.store.list_wallet_instances(false)?;
        let wallet_available = !instances.is_empty();
        let wallet_online = instances.iter().any(|instance| instance.connected);
        let default_wallet_instance_id = instances
            .iter()
            .find(|instance| instance.connected && instance.lock_state == LockState::Unlocked)
            .or_else(|| instances.iter().find(|instance| instance.connected))
            .map(|instance| instance.wallet_instance_id.clone());

        Ok(WalletStatusResult {
            wallet_available,
            wallet_online,
            default_wallet_instance_id,
            wallet_instances: instances.iter().map(Into::into).collect(),
        })
    }

    fn wallet_list_instances(
        &mut self,
        connected_only: bool,
    ) -> CoreResult<WalletListInstancesResult> {
        Ok(WalletListInstancesResult {
            wallet_instances: self
                .store
                .list_wallet_instances(connected_only)?
                .iter()
                .map(Into::into)
                .collect(),
        })
    }

    fn wallet_list_accounts(
        &mut self,
        wallet_instance_id: Option<&WalletInstanceId>,
        include_public_key: bool,
    ) -> CoreResult<WalletListAccountsResult> {
        self.policy.check_account_listing()?;

        let instances = self.store.list_wallet_instances(false)?;
        let accounts = self.store.list_wallet_accounts(wallet_instance_id)?;
        let mut grouped: BTreeMap<WalletInstanceId, WalletAccountGroup> = BTreeMap::new();

        for instance in instances {
            if let Some(target) = wallet_instance_id
                && instance.wallet_instance_id != *target
            {
                continue;
            }
            grouped.insert(
                instance.wallet_instance_id.clone(),
                WalletAccountGroup {
                    wallet_instance_id: instance.wallet_instance_id.clone(),
                    extension_connected: instance.connected,
                    lock_state: instance.lock_state,
                    accounts: Vec::new(),
                },
            );
        }

        for account in accounts {
            if let Some(group) = grouped.get_mut(&account.wallet_instance_id) {
                let public_key = if include_public_key {
                    account.public_key.clone()
                } else {
                    None
                };
                group.accounts.push(starmask_types::WalletAccountSummary {
                    address: account.address,
                    label: account.label,
                    public_key,
                    is_default: account.is_default,
                    is_locked: account.is_locked,
                });
            }
        }

        Ok(WalletListAccountsResult {
            wallet_instances: grouped.into_values().collect(),
        })
    }

    fn wallet_get_public_key(
        &mut self,
        address: &str,
        wallet_instance_id: Option<&WalletInstanceId>,
    ) -> CoreResult<WalletGetPublicKeyResult> {
        self.policy.check_public_key_lookup(address)?;
        let selected_wallet_instance_id =
            self.resolve_wallet_instance_for_account(address, wallet_instance_id)?;
        let wallet_instance = self.require_wallet_instance(&selected_wallet_instance_id)?;
        let account = self
            .store
            .get_wallet_account(&selected_wallet_instance_id, address)?
            .ok_or_else(|| {
                CoreError::shared(SharedErrorCode::InvalidAccount, "Account not found")
            })?;

        match account.public_key {
            Some(public_key) => Ok(WalletGetPublicKeyResult {
                wallet_instance_id: selected_wallet_instance_id,
                address: address.to_owned(),
                public_key,
                curve: Curve::Ed25519,
            }),
            None if wallet_instance.lock_state == LockState::Locked => Err(CoreError::shared(
                SharedErrorCode::WalletLocked,
                "Wallet is locked and no cached public key is available",
            )),
            None => Err(CoreError::shared(
                SharedErrorCode::ResultUnavailable,
                "Public key is not cached for this account",
            )),
        }
    }

    fn create_sign_transaction_request(
        &mut self,
        command: CreateSignTransactionCommand,
    ) -> CoreResult<CreateRequestResult> {
        self.policy.check_create_sign_transaction(&command)?;
        let payload = RequestPayload::SignTransaction(TransactionPayload {
            chain_id: command.chain_id,
            raw_txn_bcs_hex: command.raw_txn_bcs_hex,
            tx_kind: command.tx_kind,
            display_hint: command.display_hint,
            client_context: command.client_context,
        });
        self.create_request(
            command.client_request_id,
            command.account_address,
            command.wallet_instance_id,
            RequestKind::SignTransaction,
            payload,
            command.ttl_seconds,
        )
    }

    fn create_account_request(
        &mut self,
        command: CreateAccountCommand,
    ) -> CoreResult<CreateRequestResult> {
        self.policy.check_create_account(&command)?;
        let payload = RequestPayload::CreateAccount(CreateAccountPayload {
            display_hint: command.display_hint,
            client_context: command.client_context,
        });
        self.create_request(
            command.client_request_id,
            String::new(),
            Some(command.wallet_instance_id),
            RequestKind::CreateAccount,
            payload,
            command.ttl_seconds,
        )
    }

    fn create_sign_message_request(
        &mut self,
        command: CreateSignMessageCommand,
    ) -> CoreResult<CreateRequestResult> {
        self.policy.check_create_sign_message(&command)?;
        let payload = RequestPayload::SignMessage(starmask_types::MessagePayload {
            message: command.message,
            format: command.format,
            display_hint: command.display_hint,
            client_context: command.client_context,
        });
        self.create_request(
            command.client_request_id,
            command.account_address,
            command.wallet_instance_id,
            RequestKind::SignMessage,
            payload,
            command.ttl_seconds,
        )
    }

    fn create_request(
        &mut self,
        client_request_id: ClientRequestId,
        account_address: String,
        wallet_instance_id: Option<WalletInstanceId>,
        kind: RequestKind,
        payload: RequestPayload,
        ttl_seconds: Option<DurationSeconds>,
    ) -> CoreResult<CreateRequestResult> {
        let payload_hash = calculate_payload_hash(&payload)?;
        if let Some(existing) = self
            .store
            .get_request_by_client_request_id(&client_request_id)?
        {
            let signing_target_matches = existing.account_address == account_address
                && wallet_instance_id
                    .as_ref()
                    .map(|requested| requested == &existing.wallet_instance_id)
                    .unwrap_or(true);
            if existing.payload_hash == payload_hash && signing_target_matches {
                return Ok(project_request_summary(&existing));
            }
            return Err(CoreError::shared(
                SharedErrorCode::IdempotencyKeyConflict,
                "client_request_id already exists with a different payload or signing target",
            ));
        }

        let selected_wallet_instance_id =
            self.resolve_wallet_for_request(kind, &account_address, wallet_instance_id.as_ref())?;
        let now = self.clock.now();
        let ttl = self.clamp_ttl(ttl_seconds);
        let expires_at = now.checked_add_seconds(ttl).ok_or_else(|| {
            CoreError::Invariant("request expiration overflowed timestamp range".to_owned())
        })?;

        let request = RequestRecord {
            request_id: self.id_generator.new_request_id()?,
            client_request_id,
            kind,
            status: RequestStatus::Created,
            wallet_instance_id: selected_wallet_instance_id,
            account_address,
            payload_hash,
            payload,
            result: None,
            created_at: now,
            expires_at,
            updated_at: now,
            approved_at: None,
            rejected_at: None,
            cancelled_at: None,
            failed_at: None,
            result_expires_at: None,
            last_error_code: None,
            last_error_message: None,
            reject_reason_code: None,
            delivery_lease: None,
            presentation: None,
        };

        let request = self.store.insert_request(request)?;
        Ok(project_request_summary(&request))
    }

    fn get_request_status(&mut self, request_id: &RequestId) -> CoreResult<GetRequestStatusResult> {
        let request = self.store.get_request(request_id)?.ok_or_else(|| {
            CoreError::shared(SharedErrorCode::RequestNotFound, "Request not found")
        })?;
        Ok(project_request_status(&request))
    }

    fn request_has_available(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
    ) -> CoreResult<RequestHasAvailableResult> {
        let available = self
            .store
            .list_non_terminal_requests()?
            .into_iter()
            .any(|request| {
                request.wallet_instance_id == *wallet_instance_id
                    && matches!(
                        request.status,
                        RequestStatus::Created | RequestStatus::PendingUserApproval
                    )
            });
        Ok(RequestHasAvailableResult {
            wallet_instance_id: wallet_instance_id.clone(),
            available,
        })
    }

    fn cancel_request(&mut self, request_id: &RequestId) -> CoreResult<CancelRequestResult> {
        let mut request = self.store.get_request(request_id)?.ok_or_else(|| {
            CoreError::shared(SharedErrorCode::RequestNotFound, "Request not found")
        })?;

        if !request.status.is_terminal() {
            transition_request(&mut request, RequestStatus::Cancelled, self.clock.now())?;
            request.cancelled_at = Some(self.clock.now());
            request.last_error_code = Some(SharedErrorCode::RequestCancelled);
            request.last_error_message = Some("Request cancelled by caller".to_owned());
            request.delivery_lease = None;
            request.presentation = None;
            self.store.update_request(request.clone())?;
        }

        Ok(CancelRequestResult {
            request_id: request.request_id,
            status: request.status,
            error_code: request.last_error_code,
        })
    }

    fn register_extension(&mut self, command: RegisterExtensionCommand) -> CoreResult<()> {
        let mut backend_metadata = serde_json::Map::new();
        backend_metadata.insert(
            "extension_id".to_owned(),
            serde_json::Value::String(command.extension_id.clone()),
        );
        backend_metadata.insert(
            "extension_version".to_owned(),
            serde_json::Value::String(command.extension_version.clone()),
        );
        if let Some(profile_hint) = command.profile_hint.clone() {
            backend_metadata.insert(
                "profile_hint".to_owned(),
                serde_json::Value::String(profile_hint.clone()),
            );
        }

        self.register_backend(RegisterBackendCommand {
            wallet_instance_id: command.wallet_instance_id.clone(),
            backend_kind: BackendKind::StarmaskExtension,
            transport_kind: TransportKind::NativeMessaging,
            approval_surface: ApprovalSurface::BrowserUi,
            instance_label: command
                .profile_hint
                .clone()
                .unwrap_or_else(|| command.wallet_instance_id.to_string()),
            extension_id: command.extension_id,
            extension_version: command.extension_version,
            protocol_version: command.protocol_version,
            capabilities: vec![
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
            backend_metadata: serde_json::Value::Object(backend_metadata),
            profile_hint: command.profile_hint,
            lock_state: command.lock_state,
            accounts: command.accounts,
        })
    }

    fn heartbeat_extension(&mut self, command: HeartbeatExtensionCommand) -> CoreResult<()> {
        self.heartbeat_backend(HeartbeatBackendCommand {
            wallet_instance_id: command.wallet_instance_id,
            presented_request_ids: command.presented_request_ids,
            lock_state: None,
        })
    }

    fn register_backend(&mut self, command: RegisterBackendCommand) -> CoreResult<()> {
        if let Some(existing) = self
            .store
            .get_wallet_instance(&command.wallet_instance_id)?
            && existing.backend_kind != command.backend_kind
        {
            return Err(CoreError::shared(
                SharedErrorCode::InvalidBackendRegistration,
                "wallet_instance_id is already registered for a different backend kind",
            ));
        }

        let now = self.clock.now();
        let wallet_instance_id = command.wallet_instance_id.clone();
        self.store.upsert_wallet_instance(WalletInstanceRecord {
            wallet_instance_id,
            backend_kind: command.backend_kind,
            transport_kind: command.transport_kind,
            approval_surface: command.approval_surface,
            instance_label: command.instance_label,
            extension_id: command.extension_id,
            extension_version: command.extension_version,
            protocol_version: command.protocol_version,
            capabilities: canonical_capabilities(command.capabilities),
            backend_metadata: command.backend_metadata,
            profile_hint: command.profile_hint,
            lock_state: command.lock_state,
            connected: true,
            last_seen_at: now,
        })?;

        let accounts = command
            .accounts
            .into_iter()
            .map(|mut account| {
                account.last_seen_at = now;
                account
            })
            .collect();
        self.store
            .replace_wallet_accounts(&command.wallet_instance_id, accounts)?;
        Ok(())
    }

    fn heartbeat_backend(&mut self, command: HeartbeatBackendCommand) -> CoreResult<()> {
        let mut instance = self.require_wallet_instance(&command.wallet_instance_id)?;
        instance.connected = true;
        let now = self.clock.now();
        instance.last_seen_at = now;
        if let Some(lock_state) = command.lock_state {
            instance.lock_state = lock_state;
        }
        self.store.upsert_wallet_instance(instance)?;

        if command.presented_request_ids.is_empty() {
            return Ok(());
        }

        let presentation_expires_at = now
            .checked_add_seconds(self.config.presentation_lease_ttl)
            .ok_or_else(|| {
                CoreError::Invariant("presentation lease timestamp overflow".to_owned())
            })?;
        for request_id in command.presented_request_ids {
            let Some(mut request) = self.store.get_request(&request_id)? else {
                continue;
            };
            if request.wallet_instance_id != command.wallet_instance_id
                || request.status != RequestStatus::PendingUserApproval
            {
                continue;
            }
            let Some(mut presentation) = request.presentation.clone() else {
                continue;
            };
            presentation.presentation_expires_at = presentation_expires_at;
            request.presentation = Some(presentation);
            request.updated_at = now;
            self.store.update_request(request)?;
        }
        Ok(())
    }

    fn update_extension_accounts(
        &mut self,
        command: UpdateExtensionAccountsCommand,
    ) -> CoreResult<()> {
        self.update_backend_accounts(UpdateBackendAccountsCommand {
            wallet_instance_id: command.wallet_instance_id,
            lock_state: command.lock_state,
            capabilities: vec![
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
            accounts: command.accounts,
        })
    }

    fn update_backend_accounts(&mut self, command: UpdateBackendAccountsCommand) -> CoreResult<()> {
        let now = self.clock.now();
        let mut instance = self.require_wallet_instance(&command.wallet_instance_id)?;
        instance.connected = true;
        instance.lock_state = command.lock_state;
        instance.capabilities = canonical_capabilities(command.capabilities);
        instance.last_seen_at = now;
        self.store.upsert_wallet_instance(instance)?;

        let accounts = command
            .accounts
            .into_iter()
            .map(|mut account| {
                account.last_seen_at = now;
                account
            })
            .collect();
        self.store
            .replace_wallet_accounts(&command.wallet_instance_id, accounts)?;
        Ok(())
    }

    fn pull_next_request(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
    ) -> CoreResult<PullNextRequestResult> {
        self.require_wallet_instance(wallet_instance_id)?;
        let now = self.clock.now();

        if let Some(mut request) = self.find_resumable_request(wallet_instance_id)? {
            if let Some(presentation) = request.presentation.as_mut() {
                presentation.presentation_expires_at = now
                    .checked_add_seconds(self.config.presentation_lease_ttl)
                    .ok_or_else(|| {
                        CoreError::Invariant("presentation lease timestamp overflow".to_owned())
                    })?;
            }
            request.updated_at = now;
            let request = self.store.update_request(request)?;
            return Ok(PullNextRequestResult {
                wallet_instance_id: wallet_instance_id.clone(),
                request: Some(project_pulled_request(&request, true)),
            });
        }

        let delivery_lease_id = self.id_generator.new_delivery_lease_id()?;
        let lease = DeliveryLease {
            delivery_lease_id,
            delivery_lease_expires_at: self
                .clock
                .now()
                .checked_add_seconds(self.config.delivery_lease_ttl)
                .ok_or_else(|| {
                    CoreError::Invariant("delivery lease timestamp overflow".to_owned())
                })?,
        };
        let request = self
            .store
            .claim_next_request_for_wallet(wallet_instance_id, lease, now)?;
        Ok(PullNextRequestResult {
            wallet_instance_id: wallet_instance_id.clone(),
            request: request
                .as_ref()
                .map(|request| project_pulled_request(request, false)),
        })
    }

    fn mark_request_presented(
        &mut self,
        command: MarkRequestPresentedCommand,
    ) -> CoreResult<RequestPresentedResult> {
        let now = self.clock.now();
        let mut request =
            self.require_owned_request(&command.request_id, &command.wallet_instance_id)?;
        match request.status {
            RequestStatus::Dispatched => {
                let delivery_lease_id = command.delivery_lease_id.as_ref().ok_or_else(|| {
                    CoreError::shared(
                        SharedErrorCode::LeaseMismatch,
                        "delivery_lease_id is required for the first presentation",
                    )
                })?;
                validate_delivery_lease(&request, delivery_lease_id, now)?;
                transition_request(&mut request, RequestStatus::PendingUserApproval, now)?;
                request.presentation = Some(PresentationLease {
                    presentation_id: command.presentation_id,
                    presentation_expires_at: now
                        .checked_add_seconds(self.config.presentation_lease_ttl)
                        .ok_or_else(|| {
                            CoreError::Invariant("presentation lease timestamp overflow".to_owned())
                        })?,
                });
                request.delivery_lease = None;
            }
            RequestStatus::PendingUserApproval => {
                if command.delivery_lease_id.is_some() {
                    return Err(CoreError::shared(
                        SharedErrorCode::LeaseMismatch,
                        "delivery_lease_id is not valid when resuming an existing presentation",
                    ));
                }
                validate_presentation(&request, &command.presentation_id, now)?;
                if let Some(presentation) = request.presentation.as_mut() {
                    presentation.presentation_expires_at = now
                        .checked_add_seconds(self.config.presentation_lease_ttl)
                        .ok_or_else(|| {
                            CoreError::Invariant("presentation lease timestamp overflow".to_owned())
                        })?;
                }
                request.updated_at = now;
            }
            other => {
                return Err(CoreError::InvalidStateTransition {
                    from: other,
                    to: RequestStatus::PendingUserApproval,
                });
            }
        }
        request = self.store.update_request(request)?;
        Ok(RequestPresentedResult {
            request_id: request.request_id,
            status: request.status,
        })
    }

    fn resolve_request(
        &mut self,
        command: ResolveRequestCommand,
    ) -> CoreResult<RequestResolvedResult> {
        let now = self.clock.now();
        let mut request =
            self.require_owned_request(&command.request_id, &command.wallet_instance_id)?;
        validate_request_result(&request, &command.result)?;
        validate_presentation(&request, &command.presentation_id, now)?;
        transition_request(&mut request, RequestStatus::Approved, now)?;
        request.approved_at = Some(now);
        request.result_expires_at = now.checked_add_seconds(self.config.result_retention);
        request.result = Some(command.result.clone());
        request.last_error_code = None;
        request.last_error_message = None;
        request.reject_reason_code = None;
        request.delivery_lease = None;
        request.presentation = None;
        request = self.store.update_request(request)?;
        Ok(RequestResolvedResult {
            request_id: request.request_id,
            status: request.status,
            result_kind: command.result.result_kind(),
            result_expires_at: request.result_expires_at,
        })
    }

    fn reject_request(
        &mut self,
        command: RejectRequestCommand,
    ) -> CoreResult<RequestRejectedResult> {
        let now = self.clock.now();
        let mut request =
            self.require_owned_request(&command.request_id, &command.wallet_instance_id)?;

        match request.status {
            RequestStatus::PendingUserApproval => {
                let presentation_id = command.presentation_id.as_ref().ok_or_else(|| {
                    CoreError::shared(
                        SharedErrorCode::LeaseMismatch,
                        "presentation_id is required after request presentation",
                    )
                })?;
                validate_presentation(&request, presentation_id, now)?;
            }
            RequestStatus::Dispatched => {
                if command.presentation_id.is_some() {
                    return Err(CoreError::shared(
                        SharedErrorCode::LeaseMismatch,
                        "presentation_id is not valid before request presentation",
                    ));
                }
            }
            other => {
                return Err(CoreError::InvalidStateTransition {
                    from: other,
                    to: map_reject_reason(command.reason_code).0,
                });
            }
        }

        let (status, error_code, default_message) = map_reject_reason(command.reason_code);
        transition_request(&mut request, status, now)?;
        request.approved_at = None;
        request.rejected_at = (status == RequestStatus::Rejected).then_some(now);
        request.failed_at = (status == RequestStatus::Failed).then_some(now);
        request.reject_reason_code = Some(command.reason_code);
        request.last_error_code = Some(error_code);
        request.last_error_message = Some(
            command
                .message
                .unwrap_or_else(|| default_message.to_owned()),
        );
        request.delivery_lease = None;
        request.presentation = None;
        request = self.store.update_request(request)?;
        Ok(RequestRejectedResult {
            request_id: request.request_id,
            status: request.status,
            error_code,
        })
    }

    fn tick_maintenance(&mut self) -> CoreResult<TickMaintenanceResult> {
        let now = self.clock.now();
        let mut expired_requests = 0;
        let mut released_delivery_leases = 0;

        for mut request in self.store.list_non_terminal_requests()? {
            if request.expires_at <= now {
                transition_request(&mut request, RequestStatus::Expired, now)?;
                request.last_error_code = Some(SharedErrorCode::RequestExpired);
                request.last_error_message = Some("Request reached its TTL".to_owned());
                request.delivery_lease = None;
                request.presentation = None;
                self.store.update_request(request)?;
                expired_requests += 1;
                continue;
            }

            if request.status == RequestStatus::Dispatched
                && request
                    .delivery_lease
                    .as_ref()
                    .is_some_and(|lease| lease.delivery_lease_expires_at <= now)
            {
                transition_request(&mut request, RequestStatus::Created, now)?;
                request.delivery_lease = None;
                request.presentation = None;
                self.store.update_request(request)?;
                released_delivery_leases += 1;
                continue;
            }

            if request.status == RequestStatus::PendingUserApproval
                && request
                    .presentation
                    .as_ref()
                    .is_some_and(|presentation| presentation.presentation_expires_at <= now)
            {
                transition_request(&mut request, RequestStatus::Created, now)?;
                request.delivery_lease = None;
                request.presentation = None;
                self.store.update_request(request)?;
            }
        }

        for mut wallet_instance in self.store.list_wallet_instances(true)? {
            if is_wallet_stale(
                wallet_instance.last_seen_at,
                now,
                self.config.wallet_offline_after,
            ) {
                wallet_instance.connected = false;
                self.store.upsert_wallet_instance(wallet_instance)?;
            }
        }

        let mut evicted_results = 0;
        for mut request in self
            .store
            .list_terminal_requests_with_expired_results(now)?
        {
            if request.result.take().is_some() {
                request.result_expires_at = None;
                self.store.update_request(request)?;
                evicted_results += 1;
            }
        }

        Ok(TickMaintenanceResult {
            expired_requests,
            released_delivery_leases,
            evicted_results,
        })
    }

    fn clamp_ttl(&self, ttl_seconds: Option<DurationSeconds>) -> DurationSeconds {
        let ttl = ttl_seconds.unwrap_or(self.config.default_request_ttl);
        DurationSeconds::new(
            ttl.as_secs()
                .max(self.config.min_request_ttl.as_secs())
                .min(self.config.max_request_ttl.as_secs()),
        )
    }

    fn find_resumable_request(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
    ) -> CoreResult<Option<RequestRecord>> {
        let now = self.clock.now();
        let request = self
            .store
            .list_non_terminal_requests()?
            .into_iter()
            .filter(|request| {
                request.wallet_instance_id == *wallet_instance_id
                    && request.status == RequestStatus::PendingUserApproval
                    && request
                        .presentation
                        .as_ref()
                        .is_some_and(|presentation| presentation.presentation_expires_at > now)
            })
            .min_by_key(|request| request.created_at);
        Ok(request)
    }

    fn require_owned_request(
        &mut self,
        request_id: &RequestId,
        wallet_instance_id: &WalletInstanceId,
    ) -> CoreResult<RequestRecord> {
        let request = self.store.get_request(request_id)?.ok_or_else(|| {
            CoreError::shared(SharedErrorCode::RequestNotFound, "Request not found")
        })?;
        if request.wallet_instance_id != *wallet_instance_id {
            return Err(CoreError::shared(
                SharedErrorCode::RequestNotOwned,
                "wallet instance does not own this request",
            ));
        }
        Ok(request)
    }

    fn require_wallet_instance(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
    ) -> CoreResult<WalletInstanceRecord> {
        self.store
            .get_wallet_instance(wallet_instance_id)?
            .ok_or_else(|| {
                CoreError::shared(
                    SharedErrorCode::WalletInstanceNotFound,
                    "Wallet instance was not found",
                )
            })
    }

    fn resolve_wallet_for_signing(
        &mut self,
        kind: RequestKind,
        address: &str,
        wallet_instance_id: Option<&WalletInstanceId>,
    ) -> CoreResult<WalletInstanceId> {
        let selected = self.resolve_wallet_instance_for_account(address, wallet_instance_id)?;
        let wallet = self.require_wallet_instance(&selected)?;
        let capability = capability_for_request(kind);
        if !wallet.capabilities.contains(&capability) {
            return Err(CoreError::shared(
                SharedErrorCode::UnsupportedOperation,
                format!("Selected wallet instance does not support {capability:?}"),
            ));
        }
        if !wallet.connected {
            return Err(CoreError::shared(
                SharedErrorCode::WalletUnavailable,
                "Selected wallet instance is not connected",
            ));
        }
        if wallet.lock_state != LockState::Unlocked
            && !wallet.capabilities.contains(&WalletCapability::Unlock)
        {
            return Err(CoreError::shared(
                SharedErrorCode::WalletLocked,
                "Selected wallet instance is locked",
            ));
        }
        let account = self
            .store
            .get_wallet_account(&selected, address)?
            .ok_or_else(|| {
                CoreError::shared(
                    SharedErrorCode::InvalidAccount,
                    "Account is not exposed by the selected wallet instance",
                )
            })?;
        if account.is_read_only {
            return Err(CoreError::shared(
                SharedErrorCode::UnsupportedOperation,
                "Selected account is read-only and cannot sign",
            ));
        }
        Ok(selected)
    }

    fn resolve_wallet_for_request(
        &mut self,
        kind: RequestKind,
        address: &str,
        wallet_instance_id: Option<&WalletInstanceId>,
    ) -> CoreResult<WalletInstanceId> {
        match kind {
            RequestKind::CreateAccount => {
                let wallet_instance_id = wallet_instance_id.ok_or_else(|| {
                    CoreError::shared(
                        SharedErrorCode::WalletSelectionRequired,
                        "wallet_instance_id is required for create_account requests",
                    )
                })?;
                let wallet = self.require_wallet_instance(wallet_instance_id)?;
                if !wallet
                    .capabilities
                    .contains(&WalletCapability::CreateAccount)
                {
                    return Err(CoreError::shared(
                        SharedErrorCode::UnsupportedOperation,
                        "Selected wallet instance does not support CreateAccount",
                    ));
                }
                if !wallet.connected {
                    return Err(CoreError::shared(
                        SharedErrorCode::WalletUnavailable,
                        "Selected wallet instance is not connected",
                    ));
                }
                Ok(wallet_instance_id.clone())
            }
            RequestKind::SignTransaction | RequestKind::SignMessage => {
                self.resolve_wallet_for_signing(kind, address, wallet_instance_id)
            }
        }
    }

    fn resolve_wallet_instance_for_account(
        &mut self,
        address: &str,
        wallet_instance_id: Option<&WalletInstanceId>,
    ) -> CoreResult<WalletInstanceId> {
        if let Some(wallet_instance_id) = wallet_instance_id {
            let account = self.store.get_wallet_account(wallet_instance_id, address)?;
            if account.is_none() {
                return Err(CoreError::shared(
                    SharedErrorCode::InvalidAccount,
                    "Account is not exposed by the selected wallet instance",
                ));
            }
            return Ok(wallet_instance_id.clone());
        }

        let accounts = self.store.list_wallet_accounts(None)?;
        let mut matches: Vec<WalletInstanceId> = accounts
            .into_iter()
            .filter(|account| account.address == address)
            .map(|account| account.wallet_instance_id)
            .collect();
        matches.sort();
        matches.dedup();

        match matches.len() {
            0 => Err(CoreError::shared(
                SharedErrorCode::InvalidAccount,
                "Account is not exposed by any known wallet instance",
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(CoreError::shared(
                SharedErrorCode::WalletSelectionRequired,
                "Multiple wallet instances expose the requested account",
            )),
        }
    }
}

fn calculate_payload_hash(payload: &RequestPayload) -> CoreResult<PayloadHash> {
    use sha2::{Digest, Sha256};

    let encoded = serde_json::to_vec(payload)
        .map_err(|error| CoreError::Invariant(format!("payload serialization failed: {error}")))?;
    let digest = Sha256::digest(encoded);
    PayloadHash::new(hex::encode(digest)).map_err(|error| {
        CoreError::Invariant(format!(
            "payload hash generation produced an invalid id: {error}"
        ))
    })
}

fn is_wallet_stale(
    last_seen_at: TimestampMs,
    now: TimestampMs,
    stale_after: DurationSeconds,
) -> bool {
    let threshold_millis = stale_after
        .as_secs()
        .checked_mul(1000)
        .and_then(|value| i64::try_from(value).ok())
        .unwrap_or(i64::MAX);
    now.as_millis().saturating_sub(last_seen_at.as_millis()) >= threshold_millis
}

fn capability_for_request(kind: RequestKind) -> WalletCapability {
    match kind {
        RequestKind::SignTransaction => WalletCapability::SignTransaction,
        RequestKind::SignMessage => WalletCapability::SignMessage,
        RequestKind::CreateAccount => WalletCapability::CreateAccount,
    }
}

fn canonical_capabilities(mut capabilities: Vec<WalletCapability>) -> Vec<WalletCapability> {
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn project_request_summary(request: &RequestRecord) -> CreateRequestResult {
    CreateRequestResult {
        request_id: request.request_id.clone(),
        client_request_id: request.client_request_id.clone(),
        kind: request.kind,
        status: request.status,
        wallet_instance_id: request.wallet_instance_id.clone(),
        created_at: request.created_at,
        expires_at: request.expires_at,
    }
}

fn project_request_status(request: &RequestRecord) -> GetRequestStatusResult {
    GetRequestStatusResult {
        request_id: request.request_id.clone(),
        client_request_id: request.client_request_id.clone(),
        kind: request.kind,
        status: request.status,
        wallet_instance_id: request.wallet_instance_id.clone(),
        created_at: request.created_at,
        expires_at: request.expires_at,
        result_kind: request
            .result
            .as_ref()
            .map(RequestResult::result_kind)
            .unwrap_or_else(|| request.kind.expected_result_kind()),
        result_available: request.result.is_some(),
        result_expires_at: request.result_expires_at,
        error_code: request.last_error_code,
        error_message: request.last_error_message.clone(),
        result: request.result.clone(),
    }
}

fn project_pulled_request(request: &RequestRecord, resume_required: bool) -> PulledRequest {
    let (display_hint, client_context, raw_txn_bcs_hex, message, message_format) =
        match &request.payload {
            RequestPayload::SignTransaction(payload) => (
                payload.display_hint.clone(),
                payload.client_context.clone(),
                Some(payload.raw_txn_bcs_hex.clone()),
                None,
                None,
            ),
            RequestPayload::SignMessage(payload) => (
                payload.display_hint.clone(),
                payload.client_context.clone(),
                None,
                Some(payload.message.clone()),
                Some(payload.format),
            ),
            RequestPayload::CreateAccount(payload) => (
                payload.display_hint.clone(),
                payload.client_context.clone(),
                None,
                None,
                None,
            ),
        };

    PulledRequest {
        request_id: request.request_id.clone(),
        client_request_id: request.client_request_id.clone(),
        kind: request.kind,
        account_address: request.account_address.clone(),
        payload_hash: request.payload_hash.clone(),
        display_hint,
        client_context,
        resume_required,
        delivery_lease_id: (!resume_required)
            .then(|| {
                request
                    .delivery_lease
                    .as_ref()
                    .map(|lease| lease.delivery_lease_id.clone())
            })
            .flatten(),
        lease_expires_at: (!resume_required)
            .then(|| {
                request
                    .delivery_lease
                    .as_ref()
                    .map(|lease| lease.delivery_lease_expires_at)
            })
            .flatten(),
        presentation_id: resume_required
            .then(|| {
                request
                    .presentation
                    .as_ref()
                    .map(|presentation| presentation.presentation_id.clone())
            })
            .flatten(),
        presentation_expires_at: resume_required
            .then(|| {
                request
                    .presentation
                    .as_ref()
                    .map(|presentation| presentation.presentation_expires_at)
            })
            .flatten(),
        raw_txn_bcs_hex,
        message,
        message_format,
    }
}

fn validate_delivery_lease(
    request: &RequestRecord,
    delivery_lease_id: &starmask_types::DeliveryLeaseId,
    now: TimestampMs,
) -> CoreResult<()> {
    let Some(lease) = request.delivery_lease.as_ref() else {
        return Err(CoreError::shared(
            SharedErrorCode::LeaseMismatch,
            "request does not have an active delivery lease",
        ));
    };
    if lease.delivery_lease_id != *delivery_lease_id {
        return Err(CoreError::shared(
            SharedErrorCode::LeaseMismatch,
            "delivery lease does not match the active claim",
        ));
    }
    if lease.delivery_lease_expires_at <= now {
        return Err(CoreError::shared(
            SharedErrorCode::LeaseMismatch,
            "delivery lease already expired",
        ));
    }
    Ok(())
}

fn validate_presentation(
    request: &RequestRecord,
    presentation_id: &starmask_types::PresentationId,
    now: TimestampMs,
) -> CoreResult<()> {
    if request.status != RequestStatus::PendingUserApproval {
        return Err(CoreError::InvalidStateTransition {
            from: request.status,
            to: RequestStatus::Approved,
        });
    }
    let Some(presentation) = request.presentation.as_ref() else {
        return Err(CoreError::shared(
            SharedErrorCode::LeaseMismatch,
            "request does not have an active presentation lease",
        ));
    };
    if presentation.presentation_id != *presentation_id {
        return Err(CoreError::shared(
            SharedErrorCode::LeaseMismatch,
            "presentation id does not match the active request presentation",
        ));
    }
    if presentation.presentation_expires_at <= now {
        return Err(CoreError::shared(
            SharedErrorCode::LeaseMismatch,
            "presentation lease already expired",
        ));
    }
    Ok(())
}

fn validate_request_result(request: &RequestRecord, result: &RequestResult) -> CoreResult<()> {
    let expected = request.kind.expected_result_kind();
    let actual = result.result_kind();
    if expected != actual {
        return Err(CoreError::Validation(format!(
            "request expects result kind {expected:?} but received {actual:?}"
        )));
    }
    Ok(())
}

fn map_reject_reason(reason: RejectReasonCode) -> (RequestStatus, SharedErrorCode, &'static str) {
    match reason {
        RejectReasonCode::RequestRejected => (
            RequestStatus::Rejected,
            SharedErrorCode::RequestRejected,
            "Request rejected by user",
        ),
        RejectReasonCode::RequestExpired => (
            RequestStatus::Expired,
            SharedErrorCode::RequestExpired,
            "Request expired before approval completed",
        ),
        RejectReasonCode::WalletLocked => (
            RequestStatus::Failed,
            SharedErrorCode::WalletLocked,
            "Wallet became locked before signing completed",
        ),
        RejectReasonCode::UnsupportedOperation => (
            RequestStatus::Failed,
            SharedErrorCode::UnsupportedOperation,
            "Wallet cannot safely approve this operation",
        ),
        RejectReasonCode::InvalidTransactionPayload => (
            RequestStatus::Failed,
            SharedErrorCode::InvalidTransactionPayload,
            "Wallet rejected an invalid transaction payload",
        ),
        RejectReasonCode::InvalidMessagePayload => (
            RequestStatus::Failed,
            SharedErrorCode::InvalidMessagePayload,
            "Wallet rejected an invalid message payload",
        ),
        RejectReasonCode::BackendUnavailable => (
            RequestStatus::Failed,
            SharedErrorCode::BackendUnavailable,
            "Backend became unavailable before signing completed",
        ),
        RejectReasonCode::BackendPolicyBlocked => (
            RequestStatus::Failed,
            SharedErrorCode::BackendPolicyBlocked,
            "Backend policy blocked the request",
        ),
        RejectReasonCode::InternalError => (
            RequestStatus::Failed,
            SharedErrorCode::InternalBridgeError,
            "Wallet failed to complete the request",
        ),
    }
}

fn transition_request(
    request: &mut RequestRecord,
    next: RequestStatus,
    now: TimestampMs,
) -> CoreResult<()> {
    if request.status == next {
        return Ok(());
    }
    if !request.status.can_transition_to(next) {
        return Err(CoreError::InvalidStateTransition {
            from: request.status,
            to: next,
        });
    }
    debug!(
        request_id = %request.request_id,
        from = ?request.status,
        to = ?next,
        "transitioning request lifecycle state",
    );
    request.status = next;
    request.updated_at = now;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;

    use starmask_types::{
        ApprovalSurface, BackendKind, ClientRequestId, DeliveryLease, DeliveryLeaseId,
        DurationSeconds, LockState, MessageFormat, PresentationId, RejectReasonCode, RequestKind,
        RequestPayload, RequestRecord, RequestStatus, SharedErrorCode, TimestampMs,
        TransactionPayload, TransportKind, WalletAccountRecord, WalletCapability, WalletInstanceId,
        WalletInstanceRecord,
    };

    use crate::{
        commands::{
            CoordinatorCommand, CreateAccountCommand, CreateSignMessageCommand,
            CreateSignTransactionCommand, MarkRequestPresentedCommand, RegisterBackendCommand,
            RegisterExtensionCommand, RejectRequestCommand, ResolveRequestCommand,
            UpdateBackendAccountsCommand, UpdateExtensionAccountsCommand,
        },
        policy::AllowAllPolicy,
        repo::{RepositoryError, RequestRepository, WalletRepository},
    };

    use super::{Clock, Coordinator, CoordinatorConfig, CoordinatorResponse, IdGenerator};

    #[derive(Default)]
    struct MemoryStore {
        requests: HashMap<String, RequestRecord>,
        wallet_instances: HashMap<String, WalletInstanceRecord>,
        wallet_accounts: HashMap<(String, String), WalletAccountRecord>,
    }

    impl RequestRepository for MemoryStore {
        fn get_request(
            &mut self,
            request_id: &starmask_types::RequestId,
        ) -> Result<Option<RequestRecord>, RepositoryError> {
            Ok(self.requests.get(request_id.as_str()).cloned())
        }

        fn get_request_by_client_request_id(
            &mut self,
            client_request_id: &ClientRequestId,
        ) -> Result<Option<RequestRecord>, RepositoryError> {
            Ok(self
                .requests
                .values()
                .find(|request| request.client_request_id == *client_request_id)
                .cloned())
        }

        fn insert_request(
            &mut self,
            request: RequestRecord,
        ) -> Result<RequestRecord, RepositoryError> {
            self.requests
                .insert(request.request_id.to_string(), request.clone());
            Ok(request)
        }

        fn update_request(
            &mut self,
            request: RequestRecord,
        ) -> Result<RequestRecord, RepositoryError> {
            self.requests
                .insert(request.request_id.to_string(), request.clone());
            Ok(request)
        }

        fn claim_next_request_for_wallet(
            &mut self,
            wallet_instance_id: &WalletInstanceId,
            delivery_lease: DeliveryLease,
            now: TimestampMs,
        ) -> Result<Option<RequestRecord>, RepositoryError> {
            let next_request_id = self
                .requests
                .values()
                .filter(|request| {
                    request.wallet_instance_id == *wallet_instance_id
                        && request.status == RequestStatus::Created
                })
                .min_by_key(|request| request.created_at)
                .map(|request| request.request_id.clone());

            let Some(request_id) = next_request_id else {
                return Ok(None);
            };

            let request = self.requests.get_mut(request_id.as_str()).ok_or_else(|| {
                RepositoryError::Storage("request disappeared during claim".to_owned())
            })?;
            request.status = RequestStatus::Dispatched;
            request.updated_at = now;
            request.delivery_lease = Some(delivery_lease);
            Ok(Some(request.clone()))
        }

        fn list_non_terminal_requests(&mut self) -> Result<Vec<RequestRecord>, RepositoryError> {
            Ok(self
                .requests
                .values()
                .filter(|request| !request.status.is_terminal())
                .cloned()
                .collect())
        }

        fn list_terminal_requests_with_expired_results(
            &mut self,
            now: TimestampMs,
        ) -> Result<Vec<RequestRecord>, RepositoryError> {
            Ok(self
                .requests
                .values()
                .filter(|request| {
                    request.status.is_terminal()
                        && request
                            .result_expires_at
                            .is_some_and(|expires_at| expires_at <= now)
                })
                .cloned()
                .collect())
        }
    }

    impl WalletRepository for MemoryStore {
        fn get_wallet_instance(
            &mut self,
            wallet_instance_id: &WalletInstanceId,
        ) -> Result<Option<WalletInstanceRecord>, RepositoryError> {
            Ok(self
                .wallet_instances
                .get(wallet_instance_id.as_str())
                .cloned())
        }

        fn upsert_wallet_instance(
            &mut self,
            wallet_instance: WalletInstanceRecord,
        ) -> Result<(), RepositoryError> {
            self.wallet_instances.insert(
                wallet_instance.wallet_instance_id.to_string(),
                wallet_instance,
            );
            Ok(())
        }

        fn list_wallet_instances(
            &mut self,
            connected_only: bool,
        ) -> Result<Vec<WalletInstanceRecord>, RepositoryError> {
            Ok(self
                .wallet_instances
                .values()
                .filter(|instance| !connected_only || instance.connected)
                .cloned()
                .collect())
        }

        fn replace_wallet_accounts(
            &mut self,
            wallet_instance_id: &WalletInstanceId,
            accounts: Vec<WalletAccountRecord>,
        ) -> Result<(), RepositoryError> {
            self.wallet_accounts
                .retain(|(instance_id, _), _| instance_id != wallet_instance_id.as_str());
            for account in accounts {
                self.wallet_accounts.insert(
                    (
                        account.wallet_instance_id.to_string(),
                        account.address.clone(),
                    ),
                    account,
                );
            }
            Ok(())
        }

        fn list_wallet_accounts(
            &mut self,
            wallet_instance_id: Option<&WalletInstanceId>,
        ) -> Result<Vec<WalletAccountRecord>, RepositoryError> {
            Ok(self
                .wallet_accounts
                .values()
                .filter(|account| {
                    wallet_instance_id
                        .map(|target| account.wallet_instance_id == *target)
                        .unwrap_or(true)
                })
                .cloned()
                .collect())
        }

        fn get_wallet_account(
            &mut self,
            wallet_instance_id: &WalletInstanceId,
            address: &str,
        ) -> Result<Option<WalletAccountRecord>, RepositoryError> {
            Ok(self
                .wallet_accounts
                .get(&(wallet_instance_id.to_string(), address.to_owned()))
                .cloned())
        }
    }

    #[derive(Clone, Copy)]
    struct FixedClock {
        now: TimestampMs,
    }

    impl Clock for FixedClock {
        fn now(&self) -> TimestampMs {
            self.now
        }
    }

    #[derive(Default)]
    struct SequentialIds {
        next: u64,
    }

    impl IdGenerator for SequentialIds {
        fn new_request_id(&mut self) -> super::CoreResult<starmask_types::RequestId> {
            self.next += 1;
            starmask_types::RequestId::new(format!("req-{}", self.next))
                .map_err(|error| super::CoreError::Invariant(error.to_string()))
        }

        fn new_delivery_lease_id(&mut self) -> super::CoreResult<starmask_types::DeliveryLeaseId> {
            self.next += 1;
            starmask_types::DeliveryLeaseId::new(format!("lease-{}", self.next))
                .map_err(|error| super::CoreError::Invariant(error.to_string()))
        }
    }

    fn build_coordinator() -> Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds> {
        Coordinator::new(
            MemoryStore::default(),
            AllowAllPolicy,
            FixedClock {
                now: TimestampMs::from_millis(1_710_000_000_000),
            },
            SequentialIds::default(),
            CoordinatorConfig::default(),
        )
    }

    fn wallet_account(
        wallet_instance_id: &WalletInstanceId,
        address: &str,
        is_default: bool,
    ) -> WalletAccountRecord {
        WalletAccountRecord {
            wallet_instance_id: wallet_instance_id.clone(),
            address: address.to_owned(),
            label: None,
            public_key: None,
            is_default,
            is_read_only: false,
            is_locked: false,
            last_seen_at: TimestampMs::from_millis(0),
        }
    }

    fn register_wallet(
        coordinator: &mut Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds>,
        wallet_instance_id: &WalletInstanceId,
        lock_state: LockState,
        accounts: Vec<WalletAccountRecord>,
    ) {
        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state,
                    accounts,
                },
            ))
            .unwrap();
    }

    fn create_sign_transaction(
        coordinator: &mut Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds>,
        client_request_id: &str,
        wallet_instance_id: &WalletInstanceId,
        ttl_seconds: Option<DurationSeconds>,
    ) -> starmask_types::CreateRequestResult {
        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new(client_request_id).unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: None,
                    client_context: None,
                    ttl_seconds,
                },
            ))
            .unwrap();

        let CoordinatorResponse::RequestCreated(created) = created else {
            panic!("unexpected response");
        };
        created
    }

    fn create_sign_message(
        coordinator: &mut Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds>,
        client_request_id: &str,
        wallet_instance_id: &WalletInstanceId,
        ttl_seconds: Option<DurationSeconds>,
    ) -> starmask_types::CreateRequestResult {
        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new(client_request_id).unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    message: "hello".to_owned(),
                    format: MessageFormat::Utf8,
                    display_hint: None,
                    client_context: None,
                    ttl_seconds,
                },
            ))
            .unwrap();

        let CoordinatorResponse::RequestCreated(created) = created else {
            panic!("unexpected response");
        };
        created
    }

    fn create_account_request(
        coordinator: &mut Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds>,
        client_request_id: &str,
        wallet_instance_id: &WalletInstanceId,
        ttl_seconds: Option<DurationSeconds>,
    ) -> starmask_types::CreateRequestResult {
        let created = coordinator
            .dispatch(CoordinatorCommand::CreateAccount(CreateAccountCommand {
                client_request_id: ClientRequestId::new(client_request_id).unwrap(),
                wallet_instance_id: wallet_instance_id.clone(),
                display_hint: Some("Create account".to_owned()),
                client_context: Some("codex".to_owned()),
                ttl_seconds,
            }))
            .unwrap();

        let CoordinatorResponse::RequestCreated(created) = created else {
            panic!("unexpected response");
        };
        created
    }

    fn pull_next_request(
        coordinator: &mut Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds>,
        wallet_instance_id: &WalletInstanceId,
    ) -> starmask_types::PulledRequest {
        let pulled = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();
        let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
            panic!("unexpected response");
        };
        pulled.request.expect("request should be available")
    }

    fn mark_presented(
        coordinator: &mut Coordinator<MemoryStore, AllowAllPolicy, FixedClock, SequentialIds>,
        request_id: &starmask_types::RequestId,
        wallet_instance_id: &WalletInstanceId,
        delivery_lease_id: DeliveryLeaseId,
        presentation_id: PresentationId,
    ) {
        coordinator
            .dispatch(CoordinatorCommand::MarkRequestPresented(
                MarkRequestPresentedCommand {
                    request_id: request_id.clone(),
                    wallet_instance_id: wallet_instance_id.clone(),
                    delivery_lease_id: Some(delivery_lease_id),
                    presentation_id,
                },
            ))
            .unwrap();
    }

    #[test]
    fn public_key_lookup_allows_read_only_accounts() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-read-only").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: Some("0xpub".to_owned()),
                        is_default: true,
                        is_read_only: true,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let response = coordinator
            .dispatch(CoordinatorCommand::WalletGetPublicKey {
                address: "0x1".to_owned(),
                wallet_instance_id: Some(wallet_instance_id.clone()),
            })
            .unwrap();

        let CoordinatorResponse::WalletPublicKey(result) = response else {
            panic!("unexpected response");
        };
        assert_eq!(result.wallet_instance_id, wallet_instance_id);
        assert_eq!(result.public_key, "0xpub");
    }

    #[test]
    fn public_key_lookup_keeps_read_only_accounts_in_ambiguity_resolution() {
        let mut coordinator = build_coordinator();
        let writable_wallet_instance_id = WalletInstanceId::new("wallet-writable").unwrap();
        let read_only_wallet_instance_id = WalletInstanceId::new("wallet-read-only").unwrap();

        for wallet_instance_id in [&writable_wallet_instance_id, &read_only_wallet_instance_id] {
            coordinator
                .dispatch(CoordinatorCommand::RegisterExtension(
                    RegisterExtensionCommand {
                        wallet_instance_id: wallet_instance_id.clone(),
                        extension_id: "ext".to_owned(),
                        extension_version: "1.0.0".to_owned(),
                        protocol_version: 1,
                        profile_hint: None,
                        lock_state: LockState::Unlocked,
                        accounts: Vec::new(),
                    },
                ))
                .unwrap();
        }

        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: writable_wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: writable_wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: Some("0xpub-w".to_owned()),
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: read_only_wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: read_only_wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: Some("0xpub-r".to_owned()),
                        is_default: true,
                        is_read_only: true,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let error = coordinator
            .dispatch(CoordinatorCommand::WalletGetPublicKey {
                address: "0x1".to_owned(),
                wallet_instance_id: None,
            })
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "wallet_selection_required: Multiple wallet instances expose the requested account"
        );
    }

    #[test]
    fn signing_still_rejects_read_only_accounts_after_shared_resolution() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-read-only-sign").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: Some("0xpub".to_owned()),
                        is_default: true,
                        is_read_only: true,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new("client-read-only").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id),
                    message: "hello".to_owned(),
                    format: MessageFormat::Utf8,
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unsupported_operation: Selected account is read-only and cannot sign"
        );
    }

    #[test]
    fn create_request_is_idempotent_for_same_payload() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: Some("Default".to_owned()),
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: Some("Primary".to_owned()),
                        public_key: Some("0xpub".to_owned()),
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let command = CreateSignTransactionCommand {
            client_request_id: ClientRequestId::new("client-1").unwrap(),
            account_address: "0x1".to_owned(),
            wallet_instance_id: Some(wallet_instance_id.clone()),
            chain_id: 251,
            raw_txn_bcs_hex: "0xabc".to_owned(),
            tx_kind: "transfer".to_owned(),
            display_hint: None,
            client_context: None,
            ttl_seconds: None,
        };

        let first = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(command.clone()))
            .unwrap();
        let second = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(command))
            .unwrap();

        let CoordinatorResponse::RequestCreated(first) = first else {
            panic!("unexpected response");
        };
        let CoordinatorResponse::RequestCreated(second) = second else {
            panic!("unexpected response");
        };

        assert_eq!(first, second);
    }

    #[test]
    fn request_moves_to_approved_after_present_and_resolve() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new("client-2").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap();

        let request_id = match created {
            CoordinatorResponse::RequestCreated(result) => result.request_id,
            other => panic!("unexpected response: {other:?}"),
        };

        coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();

        let presentation_id = starmask_types::PresentationId::new("presentation-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::MarkRequestPresented(
                MarkRequestPresentedCommand {
                    request_id: request_id.clone(),
                    wallet_instance_id: wallet_instance_id.clone(),
                    delivery_lease_id: Some(
                        starmask_types::DeliveryLeaseId::new("lease-2").unwrap(),
                    ),
                    presentation_id: presentation_id.clone(),
                },
            ))
            .unwrap();

        coordinator
            .dispatch(CoordinatorCommand::ResolveRequest(ResolveRequestCommand {
                request_id: request_id.clone(),
                wallet_instance_id,
                presentation_id,
                result: starmask_types::RequestResult::SignedTransaction {
                    signed_txn_bcs_hex: "0xsigned".to_owned(),
                },
            }))
            .unwrap();

        let status = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus { request_id })
            .unwrap();

        let CoordinatorResponse::RequestStatus(status) = status else {
            panic!("unexpected response");
        };
        assert_eq!(status.status, RequestStatus::Approved);
        assert_eq!(status.result_available, true);
        assert_eq!(
            status.result,
            Some(starmask_types::RequestResult::SignedTransaction {
                signed_txn_bcs_hex: "0xsigned".to_owned()
            })
        );
    }

    #[test]
    fn wallet_selection_is_required_for_ambiguous_account() {
        let mut coordinator = build_coordinator();

        for wallet_name in ["wallet-1", "wallet-2"] {
            let wallet_instance_id = WalletInstanceId::new(wallet_name).unwrap();
            coordinator
                .dispatch(CoordinatorCommand::RegisterExtension(
                    RegisterExtensionCommand {
                        wallet_instance_id: wallet_instance_id.clone(),
                        extension_id: "ext".to_owned(),
                        extension_version: "1.0.0".to_owned(),
                        protocol_version: 1,
                        profile_hint: None,
                        lock_state: LockState::Unlocked,
                        accounts: Vec::new(),
                    },
                ))
                .unwrap();
            coordinator
                .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                    UpdateExtensionAccountsCommand {
                        wallet_instance_id: wallet_instance_id.clone(),
                        lock_state: LockState::Unlocked,
                        accounts: vec![WalletAccountRecord {
                            wallet_instance_id,
                            address: "0x1".to_owned(),
                            label: None,
                            public_key: None,
                            is_default: true,
                            is_read_only: false,
                            is_locked: false,
                            last_seen_at: TimestampMs::from_millis(0),
                        }],
                    },
                ))
                .unwrap();
        }

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new("client-3").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: None,
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "wallet_selection_required: Multiple wallet instances expose the requested account"
        );
    }

    #[test]
    fn payload_hash_is_stable_for_same_payload() {
        let first =
            super::calculate_payload_hash(&RequestPayload::SignTransaction(TransactionPayload {
                chain_id: 251,
                raw_txn_bcs_hex: "0x1".to_owned(),
                tx_kind: "transfer".to_owned(),
                display_hint: None,
                client_context: None,
            }))
            .unwrap();
        let second =
            super::calculate_payload_hash(&RequestPayload::SignTransaction(TransactionPayload {
                chain_id: 251,
                raw_txn_bcs_hex: "0x1".to_owned(),
                tx_kind: "transfer".to_owned(),
                display_hint: None,
                client_context: None,
            }))
            .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn pull_next_resumes_presented_request_for_same_wallet_instance() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let delivery_lease_id = DeliveryLeaseId::new("lease-2").unwrap();
        let presentation_id = PresentationId::new("presentation-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new("client-resume").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: Some("Transfer".to_owned()),
                    client_context: Some("codex".to_owned()),
                    ttl_seconds: None,
                },
            ))
            .unwrap();
        let request_id = match created {
            CoordinatorResponse::RequestCreated(result) => result.request_id,
            other => panic!("unexpected response: {other:?}"),
        };

        coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::MarkRequestPresented(
                MarkRequestPresentedCommand {
                    request_id: request_id.clone(),
                    wallet_instance_id: wallet_instance_id.clone(),
                    delivery_lease_id: Some(delivery_lease_id.clone()),
                    presentation_id: presentation_id.clone(),
                },
            ))
            .unwrap();

        let resumed = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();

        let CoordinatorResponse::PullNextRequest(resumed) = resumed else {
            panic!("unexpected response");
        };
        let resumed_request = resumed.request.expect("resumed request");
        assert_eq!(resumed_request.request_id, request_id);
        assert_eq!(resumed_request.resume_required, true);
        assert_eq!(resumed_request.delivery_lease_id, None);
        assert_eq!(resumed_request.presentation_id, Some(presentation_id));
    }

    #[test]
    fn reject_wallet_locked_moves_request_to_failed() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let delivery_lease_id = DeliveryLeaseId::new("lease-2").unwrap();
        let presentation_id = PresentationId::new("presentation-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new("client-reject").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap();
        let request_id = match created {
            CoordinatorResponse::RequestCreated(result) => result.request_id,
            other => panic!("unexpected response: {other:?}"),
        };

        coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::MarkRequestPresented(
                MarkRequestPresentedCommand {
                    request_id: request_id.clone(),
                    wallet_instance_id: wallet_instance_id.clone(),
                    delivery_lease_id: Some(delivery_lease_id),
                    presentation_id: presentation_id.clone(),
                },
            ))
            .unwrap();

        let rejected = coordinator
            .dispatch(CoordinatorCommand::RejectRequest(RejectRequestCommand {
                request_id: request_id.clone(),
                wallet_instance_id: wallet_instance_id.clone(),
                presentation_id: Some(presentation_id),
                reason_code: RejectReasonCode::WalletLocked,
                message: None,
            }))
            .unwrap();
        let CoordinatorResponse::RequestRejected(rejected) = rejected else {
            panic!("unexpected response");
        };
        assert_eq!(rejected.status, RequestStatus::Failed);
        assert_eq!(rejected.error_code, SharedErrorCode::WalletLocked);

        let status = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus { request_id })
            .unwrap();
        let CoordinatorResponse::RequestStatus(status) = status else {
            panic!("unexpected response");
        };
        assert_eq!(status.status, RequestStatus::Failed);
        assert_eq!(status.error_code, Some(SharedErrorCode::WalletLocked));
    }

    #[test]
    fn locked_wallet_with_unlock_capability_can_create_sign_request() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-local").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterBackend(
                RegisterBackendCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    backend_kind: BackendKind::LocalAccountDir,
                    transport_kind: TransportKind::LocalSocket,
                    approval_surface: ApprovalSurface::TtyPrompt,
                    instance_label: "Local Main".to_owned(),
                    extension_id: String::new(),
                    extension_version: String::new(),
                    protocol_version: 2,
                    capabilities: vec![
                        WalletCapability::Unlock,
                        WalletCapability::GetPublicKey,
                        WalletCapability::SignMessage,
                        WalletCapability::SignTransaction,
                        WalletCapability::CreateAccount,
                    ],
                    backend_metadata: serde_json::json!({}),
                    profile_hint: None,
                    lock_state: LockState::Locked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: true,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new("client-locked-local").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    message: "hello".to_owned(),
                    format: starmask_types::MessageFormat::Utf8,
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap();

        let CoordinatorResponse::RequestCreated(created) = created else {
            panic!("unexpected response");
        };
        assert_eq!(created.wallet_instance_id, wallet_instance_id);
        assert_eq!(created.status, RequestStatus::Created);
    }

    #[test]
    fn backend_account_updates_refresh_unlock_capabilities_for_routing() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-local-updated").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterBackend(
                RegisterBackendCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    backend_kind: BackendKind::LocalAccountDir,
                    transport_kind: TransportKind::LocalSocket,
                    approval_surface: ApprovalSurface::TtyPrompt,
                    instance_label: "Local Main".to_owned(),
                    extension_id: String::new(),
                    extension_version: String::new(),
                    protocol_version: 2,
                    capabilities: vec![
                        WalletCapability::Unlock,
                        WalletCapability::GetPublicKey,
                        WalletCapability::SignMessage,
                        WalletCapability::SignTransaction,
                        WalletCapability::CreateAccount,
                    ],
                    backend_metadata: serde_json::json!({}),
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: vec![wallet_account(&wallet_instance_id, "0x1", true)],
                },
            ))
            .unwrap();

        coordinator
            .dispatch(CoordinatorCommand::UpdateBackendAccounts(
                UpdateBackendAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Locked,
                    capabilities: vec![
                        WalletCapability::GetPublicKey,
                        WalletCapability::SignMessage,
                        WalletCapability::SignTransaction,
                        WalletCapability::CreateAccount,
                    ],
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: true,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new("client-updated-local").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id),
                    message: "hello".to_owned(),
                    format: starmask_types::MessageFormat::Utf8,
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "wallet_locked: Selected wallet instance is locked"
        );
    }

    #[test]
    fn local_backend_can_create_account_request() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-local-create").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterBackend(
                RegisterBackendCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    backend_kind: BackendKind::LocalAccountDir,
                    transport_kind: TransportKind::LocalSocket,
                    approval_surface: ApprovalSurface::TtyPrompt,
                    instance_label: "Local Main".to_owned(),
                    extension_id: String::new(),
                    extension_version: String::new(),
                    protocol_version: 2,
                    capabilities: vec![
                        WalletCapability::Unlock,
                        WalletCapability::GetPublicKey,
                        WalletCapability::SignMessage,
                        WalletCapability::SignTransaction,
                        WalletCapability::CreateAccount,
                    ],
                    backend_metadata: serde_json::json!({}),
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: vec![wallet_account(&wallet_instance_id, "0x1", true)],
                },
            ))
            .unwrap();

        let created = create_account_request(
            &mut coordinator,
            "client-create-account",
            &wallet_instance_id,
            None,
        );

        assert_eq!(created.wallet_instance_id, wallet_instance_id);
        assert_eq!(created.kind, RequestKind::CreateAccount);
        assert_eq!(created.status, RequestStatus::Created);
    }

    #[test]
    fn create_account_request_requires_create_account_capability() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-no-create").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterBackend(
                RegisterBackendCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    backend_kind: BackendKind::LocalAccountDir,
                    transport_kind: TransportKind::LocalSocket,
                    approval_surface: ApprovalSurface::TtyPrompt,
                    instance_label: "Local Main".to_owned(),
                    extension_id: String::new(),
                    extension_version: String::new(),
                    protocol_version: 2,
                    capabilities: vec![
                        WalletCapability::Unlock,
                        WalletCapability::GetPublicKey,
                        WalletCapability::SignMessage,
                        WalletCapability::SignTransaction,
                    ],
                    backend_metadata: serde_json::json!({}),
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: vec![wallet_account(&wallet_instance_id, "0x1", true)],
                },
            ))
            .unwrap();

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateAccount(CreateAccountCommand {
                client_request_id: ClientRequestId::new("client-no-create").unwrap(),
                wallet_instance_id,
                display_hint: None,
                client_context: None,
                ttl_seconds: None,
            }))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unsupported_operation: Selected wallet instance does not support CreateAccount"
        );
    }

    #[test]
    fn create_account_request_requires_connected_wallet_instance() {
        let mut coordinator = build_coordinator();
        coordinator.config.wallet_offline_after = DurationSeconds::new(25);
        let wallet_instance_id = WalletInstanceId::new("wallet-create-offline").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterBackend(
                RegisterBackendCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    backend_kind: BackendKind::LocalAccountDir,
                    transport_kind: TransportKind::LocalSocket,
                    approval_surface: ApprovalSurface::TtyPrompt,
                    instance_label: "Local Main".to_owned(),
                    extension_id: String::new(),
                    extension_version: String::new(),
                    protocol_version: 2,
                    capabilities: vec![
                        WalletCapability::Unlock,
                        WalletCapability::GetPublicKey,
                        WalletCapability::SignMessage,
                        WalletCapability::SignTransaction,
                        WalletCapability::CreateAccount,
                    ],
                    backend_metadata: serde_json::json!({}),
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: vec![wallet_account(&wallet_instance_id, "0x1", true)],
                },
            ))
            .unwrap();

        coordinator.clock.now = TimestampMs::from_millis(1_710_000_026_000);
        coordinator
            .dispatch(CoordinatorCommand::TickMaintenance)
            .unwrap();

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateAccount(CreateAccountCommand {
                client_request_id: ClientRequestId::new("client-create-offline").unwrap(),
                wallet_instance_id,
                display_hint: None,
                client_context: None,
                ttl_seconds: None,
            }))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "wallet_unavailable: Selected wallet instance is not connected"
        );
    }

    #[test]
    fn maintenance_marks_stale_wallet_instances_offline() {
        let mut coordinator = build_coordinator();
        coordinator.config.wallet_offline_after = DurationSeconds::new(25);

        let wallet_instance_id = WalletInstanceId::new("wallet-offline").unwrap();
        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();

        coordinator.clock.now = TimestampMs::from_millis(1_710_000_026_000);
        coordinator
            .dispatch(CoordinatorCommand::TickMaintenance)
            .unwrap();

        let status = coordinator
            .dispatch(CoordinatorCommand::WalletStatus)
            .unwrap();
        let CoordinatorResponse::WalletStatus(status) = status else {
            panic!("unexpected response");
        };
        assert!(!status.wallet_online);
        assert_eq!(status.wallet_instances.len(), 1);
        assert!(!status.wallet_instances[0].extension_connected);
    }

    #[test]
    fn create_request_rejects_idempotency_replay_with_different_account_target() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![
                        WalletAccountRecord {
                            wallet_instance_id: wallet_instance_id.clone(),
                            address: "0x1".to_owned(),
                            label: None,
                            public_key: None,
                            is_default: true,
                            is_read_only: false,
                            is_locked: false,
                            last_seen_at: TimestampMs::from_millis(0),
                        },
                        WalletAccountRecord {
                            wallet_instance_id: wallet_instance_id.clone(),
                            address: "0x2".to_owned(),
                            label: None,
                            public_key: None,
                            is_default: false,
                            is_read_only: false,
                            is_locked: false,
                            last_seen_at: TimestampMs::from_millis(0),
                        },
                    ],
                },
            ))
            .unwrap();

        coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new("client-idem-target").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    message: "hello".to_owned(),
                    format: MessageFormat::Utf8,
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap();

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new("client-idem-target").unwrap(),
                    account_address: "0x2".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id),
                    message: "hello".to_owned(),
                    format: MessageFormat::Utf8,
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "idempotency_key_conflict: client_request_id already exists with a different payload or signing target"
        );
    }

    #[test]
    fn pull_next_preserves_message_format_for_sign_message() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-msg").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        coordinator
            .dispatch(CoordinatorCommand::CreateSignMessage(
                CreateSignMessageCommand {
                    client_request_id: ClientRequestId::new("client-format").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    message: "68656c6c6f".to_owned(),
                    format: MessageFormat::Hex,
                    display_hint: Some("Hex payload".to_owned()),
                    client_context: Some("codex".to_owned()),
                    ttl_seconds: None,
                },
            ))
            .unwrap();

        let pulled = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest { wallet_instance_id })
            .unwrap();
        let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
            panic!("unexpected response");
        };
        let request = pulled.request.expect("message request");
        assert_eq!(request.message.as_deref(), Some("68656c6c6f"));
        assert_eq!(request.message_format, Some(MessageFormat::Hex));
    }

    #[test]
    fn expired_presentation_cannot_resolve_and_requeues_after_maintenance() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-expire").unwrap();
        let delivery_lease_id = DeliveryLeaseId::new("lease-2").unwrap();
        let presentation_id = PresentationId::new("presentation-1").unwrap();

        coordinator
            .dispatch(CoordinatorCommand::RegisterExtension(
                RegisterExtensionCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    extension_id: "ext".to_owned(),
                    extension_version: "1.0.0".to_owned(),
                    protocol_version: 1,
                    profile_hint: None,
                    lock_state: LockState::Unlocked,
                    accounts: Vec::new(),
                },
            ))
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::UpdateExtensionAccounts(
                UpdateExtensionAccountsCommand {
                    wallet_instance_id: wallet_instance_id.clone(),
                    lock_state: LockState::Unlocked,
                    accounts: vec![WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x1".to_owned(),
                        label: None,
                        public_key: None,
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: TimestampMs::from_millis(0),
                    }],
                },
            ))
            .unwrap();

        let created = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new("client-expire").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id.clone()),
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap();
        let request_id = match created {
            CoordinatorResponse::RequestCreated(result) => result.request_id,
            other => panic!("unexpected response: {other:?}"),
        };

        coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();
        coordinator
            .dispatch(CoordinatorCommand::MarkRequestPresented(
                MarkRequestPresentedCommand {
                    request_id: request_id.clone(),
                    wallet_instance_id: wallet_instance_id.clone(),
                    delivery_lease_id: Some(delivery_lease_id),
                    presentation_id: presentation_id.clone(),
                },
            ))
            .unwrap();

        coordinator.clock.now = TimestampMs::from_millis(1_710_000_046_000);

        let error = coordinator
            .dispatch(CoordinatorCommand::ResolveRequest(ResolveRequestCommand {
                request_id: request_id.clone(),
                wallet_instance_id: wallet_instance_id.clone(),
                presentation_id,
                result: starmask_types::RequestResult::SignedTransaction {
                    signed_txn_bcs_hex: "0xsigned".to_owned(),
                },
            }))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "lease_mismatch: presentation lease already expired"
        );

        let pulled = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();
        let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
            panic!("unexpected response");
        };
        assert!(pulled.request.is_none());

        coordinator
            .dispatch(CoordinatorCommand::TickMaintenance)
            .unwrap();

        let pulled = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest { wallet_instance_id })
            .unwrap();
        let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
            panic!("unexpected response");
        };
        let request = pulled.request.expect("request should be re-queued");
        assert_eq!(request.request_id, request_id);
        assert!(!request.resume_required);
        assert!(request.delivery_lease_id.is_some());
    }

    #[test]
    fn create_request_clamps_ttl_within_config_bounds() {
        let mut coordinator = build_coordinator();
        coordinator.config.min_request_ttl = DurationSeconds::new(30);
        coordinator.config.max_request_ttl = DurationSeconds::new(60);

        let wallet_instance_id = WalletInstanceId::new("wallet-ttl").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let low_ttl = create_sign_message(
            &mut coordinator,
            "client-ttl-low",
            &wallet_instance_id,
            Some(DurationSeconds::new(5)),
        );
        let high_ttl = create_sign_message(
            &mut coordinator,
            "client-ttl-high",
            &wallet_instance_id,
            Some(DurationSeconds::new(600)),
        );

        assert_eq!(
            low_ttl.expires_at.as_millis() - low_ttl.created_at.as_millis(),
            30_000
        );
        assert_eq!(
            high_ttl.expires_at.as_millis() - high_ttl.created_at.as_millis(),
            60_000
        );
    }

    #[test]
    fn create_request_rejects_locked_selected_wallet() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-locked").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Locked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let error = coordinator
            .dispatch(CoordinatorCommand::CreateSignTransaction(
                CreateSignTransactionCommand {
                    client_request_id: ClientRequestId::new("client-locked").unwrap(),
                    account_address: "0x1".to_owned(),
                    wallet_instance_id: Some(wallet_instance_id),
                    chain_id: 251,
                    raw_txn_bcs_hex: "0xabc".to_owned(),
                    tx_kind: "transfer".to_owned(),
                    display_hint: None,
                    client_context: None,
                    ttl_seconds: None,
                },
            ))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "wallet_locked: Selected wallet instance is locked"
        );
    }

    #[test]
    fn cancel_created_request_moves_to_cancelled() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-cancel-created").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-cancel-created",
            &wallet_instance_id,
            None,
        );

        let cancelled = coordinator
            .dispatch(CoordinatorCommand::CancelRequest {
                request_id: created.request_id.clone(),
            })
            .unwrap();
        let CoordinatorResponse::RequestCancelled(cancelled) = cancelled else {
            panic!("unexpected response");
        };
        assert_eq!(cancelled.status, RequestStatus::Cancelled);
        assert_eq!(
            cancelled.error_code,
            Some(SharedErrorCode::RequestCancelled)
        );
    }

    #[test]
    fn cancel_presented_request_moves_to_cancelled() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-cancel-presented").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-cancel-presented",
            &wallet_instance_id,
            None,
        );
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            PresentationId::new("presentation-cancel").unwrap(),
        );

        let cancelled = coordinator
            .dispatch(CoordinatorCommand::CancelRequest {
                request_id: created.request_id.clone(),
            })
            .unwrap();
        let CoordinatorResponse::RequestCancelled(cancelled) = cancelled else {
            panic!("unexpected response");
        };
        assert_eq!(cancelled.status, RequestStatus::Cancelled);
        assert_eq!(
            cancelled.error_code,
            Some(SharedErrorCode::RequestCancelled)
        );

        let status = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus {
                request_id: created.request_id,
            })
            .unwrap();
        let CoordinatorResponse::RequestStatus(status) = status else {
            panic!("unexpected response");
        };
        assert_eq!(status.status, RequestStatus::Cancelled);
    }

    #[test]
    fn request_reject_transitions_to_rejected() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-reject").unwrap();
        let presentation_id = PresentationId::new("presentation-reject").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-reject-normal",
            &wallet_instance_id,
            None,
        );
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            presentation_id.clone(),
        );

        let rejected = coordinator
            .dispatch(CoordinatorCommand::RejectRequest(RejectRequestCommand {
                request_id: created.request_id.clone(),
                wallet_instance_id: wallet_instance_id.clone(),
                presentation_id: Some(presentation_id),
                reason_code: RejectReasonCode::RequestRejected,
                message: Some("User explicitly rejected the request".to_owned()),
            }))
            .unwrap();
        let CoordinatorResponse::RequestRejected(rejected) = rejected else {
            panic!("unexpected response");
        };
        assert_eq!(rejected.status, RequestStatus::Rejected);
        assert_eq!(rejected.error_code, SharedErrorCode::RequestRejected);
    }

    #[test]
    fn delivery_lease_expiry_requeues_request() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-lease-expiry").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-lease-expiry",
            &wallet_instance_id,
            None,
        );

        coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: wallet_instance_id.clone(),
            })
            .unwrap();

        coordinator.clock.now = TimestampMs::from_millis(1_710_000_031_000);
        coordinator
            .dispatch(CoordinatorCommand::TickMaintenance)
            .unwrap();

        let pulled = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest { wallet_instance_id })
            .unwrap();
        let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
            panic!("unexpected response");
        };
        let request = pulled.request.expect("request should be re-queued");
        assert_eq!(request.request_id, created.request_id);
        assert!(!request.resume_required);
        assert!(request.delivery_lease_id.is_some());
    }

    #[test]
    fn maintenance_expires_created_request_after_ttl() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-expired-created").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-expired-created",
            &wallet_instance_id,
            Some(DurationSeconds::new(1)),
        );

        coordinator.clock.now = TimestampMs::from_millis(created.expires_at.as_millis());
        coordinator
            .dispatch(CoordinatorCommand::TickMaintenance)
            .unwrap();

        let status = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus {
                request_id: created.request_id,
            })
            .unwrap();
        let CoordinatorResponse::RequestStatus(status) = status else {
            panic!("unexpected response");
        };
        assert_eq!(status.status, RequestStatus::Expired);
        assert_eq!(status.error_code, Some(SharedErrorCode::RequestExpired));
    }

    #[test]
    fn approved_result_is_readable_multiple_times_before_retention_expiry() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-read-result").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-read-result",
            &wallet_instance_id,
            None,
        );
        let presentation_id = PresentationId::new("presentation-read-result").unwrap();
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            presentation_id.clone(),
        );
        coordinator
            .dispatch(CoordinatorCommand::ResolveRequest(ResolveRequestCommand {
                request_id: created.request_id.clone(),
                wallet_instance_id,
                presentation_id,
                result: starmask_types::RequestResult::SignedTransaction {
                    signed_txn_bcs_hex: "0xsigned".to_owned(),
                },
            }))
            .unwrap();

        let first = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus {
                request_id: created.request_id.clone(),
            })
            .unwrap();
        let second = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus {
                request_id: created.request_id,
            })
            .unwrap();

        let CoordinatorResponse::RequestStatus(first) = first else {
            panic!("unexpected response");
        };
        let CoordinatorResponse::RequestStatus(second) = second else {
            panic!("unexpected response");
        };
        assert_eq!(first.status, RequestStatus::Approved);
        assert!(first.result_available);
        assert_eq!(first.result, second.result);
        assert!(second.result_available);
    }

    #[test]
    fn result_payload_eviction_keeps_terminal_metadata() {
        let mut coordinator = build_coordinator();
        coordinator.config.result_retention = DurationSeconds::new(60);

        let wallet_instance_id = WalletInstanceId::new("wallet-evict-result").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-evict-result",
            &wallet_instance_id,
            None,
        );
        let presentation_id = PresentationId::new("presentation-evict-result").unwrap();
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            presentation_id.clone(),
        );
        coordinator
            .dispatch(CoordinatorCommand::ResolveRequest(ResolveRequestCommand {
                request_id: created.request_id.clone(),
                wallet_instance_id,
                presentation_id,
                result: starmask_types::RequestResult::SignedTransaction {
                    signed_txn_bcs_hex: "0xsigned".to_owned(),
                },
            }))
            .unwrap();

        coordinator.clock.now = TimestampMs::from_millis(1_710_000_061_000);
        coordinator
            .dispatch(CoordinatorCommand::TickMaintenance)
            .unwrap();

        let status = coordinator
            .dispatch(CoordinatorCommand::GetRequestStatus {
                request_id: created.request_id,
            })
            .unwrap();
        let CoordinatorResponse::RequestStatus(status) = status else {
            panic!("unexpected response");
        };
        assert_eq!(status.status, RequestStatus::Approved);
        assert_eq!(status.result, None);
        assert!(!status.result_available);
        assert_eq!(status.result_expires_at, None);
        assert_eq!(
            status.result_kind,
            starmask_types::ResultKind::SignedTransaction
        );
        assert_eq!(status.error_code, None);
    }

    #[test]
    fn presented_request_is_not_redelivered_to_other_wallet_instances() {
        let mut coordinator = build_coordinator();
        let wallet_instance_id = WalletInstanceId::new("wallet-owner").unwrap();
        let other_wallet_instance_id = WalletInstanceId::new("wallet-other").unwrap();
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        register_wallet(
            &mut coordinator,
            &other_wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&other_wallet_instance_id, "0x1", true)],
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-no-cross-instance",
            &wallet_instance_id,
            None,
        );
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            PresentationId::new("presentation-no-cross-instance").unwrap(),
        );

        let pulled = coordinator
            .dispatch(CoordinatorCommand::PullNextRequest {
                wallet_instance_id: other_wallet_instance_id,
            })
            .unwrap();
        let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
            panic!("unexpected response");
        };
        assert!(pulled.request.is_none());
    }
}
