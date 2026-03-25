#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use serde_json::{Value, json};
use starcoin_node_mcp_rpc::NodeRpcClient;
use starcoin_node_mcp_types::{
    CallViewFunctionInput, CallViewFunctionOutput, ChainContext, ChainStatusOutput, EffectiveProbe,
    GasUnitPriceSource, GetAccountOverviewInput, GetAccountOverviewOutput, GetBlockInput,
    GetBlockOutput, GetEventsInput, GetEventsOutput, GetTransactionInput, GetTransactionOutput,
    ListBlocksInput, ListBlocksOutput, ListModulesInput, ListModulesOutput, ListResourcesInput,
    ListResourcesOutput, Mode, NextAction, NodeHealthOutput, PreparationResult,
    PrepareContractCallInput, PreparePublishPackageInput, PrepareTransferInput,
    ResolveFunctionAbiInput, ResolveModuleAbiInput, ResolveStructAbiInput, RuntimeConfig,
    SequenceNumberSource, SharedError, SharedErrorCode, SimulateRawTransactionInput,
    SimulateRawTransactionOutput, SimulationResult, SimulationStatus, SubmissionNextAction,
    SubmissionState, SubmitSignedTransactionInput, SubmitSignedTransactionOutput, TransactionKind,
    TransactionStatusSummary, WatchTransactionInput, WatchTransactionOutput,
};
use starcoin_vm2_types::view::{
    FunctionIdView, TransactionArgumentView, TransactionRequest, TypeTagView,
};
use starcoin_vm2_vm_types::{
    account_address::AccountAddress,
    account_config::core_code_address,
    identifier::Identifier,
    language_storage::{ModuleId, TypeTag},
    on_chain_resource::ChainId,
    token::{stc::G_STC_TOKEN_CODE, token_code::TokenCode},
    transaction::{
        EntryFunction, Package, RawUserTransaction, SignedUserTransaction, TransactionPayload,
    },
    transaction_argument::{TransactionArgument, convert_txn_args},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    sync::{OwnedSemaphorePermit, RwLock, Semaphore},
    time::{sleep, timeout},
};
use tracing::warn;

#[derive(Debug)]
pub struct AppContext {
    config: RuntimeConfig,
    rpc: NodeRpcClient,
    watch_permits: Arc<Semaphore>,
    expensive_permits: Arc<Semaphore>,
    startup_probe: EffectiveProbe,
    transaction_probe: Arc<RwLock<Option<CachedProbe>>>,
    prepared_transactions: Arc<RwLock<HashMap<String, PreparedTransactionRecord>>>,
    unresolved_submissions: Arc<RwLock<HashMap<String, UnresolvedSubmission>>>,
}

#[derive(Debug, Clone)]
struct CachedProbe {
    probe: EffectiveProbe,
    observed_at: Instant,
}

#[derive(Debug, Clone)]
struct PreparedTransactionRecord {
    simulation_status: SimulationStatus,
    chain_context: ChainContext,
    recorded_at: Instant,
}

#[derive(Debug, Clone)]
struct UnresolvedSubmission {
    recorded_at: Instant,
}

#[derive(Debug)]
struct TransactionEndpointSnapshot {
    chain_context: ChainContext,
    now_seconds: u64,
}

impl AppContext {
    pub async fn bootstrap(config: RuntimeConfig) -> anyhow::Result<Self> {
        let rpc = NodeRpcClient::new(&config)?;
        let startup_probe = timeout(
            config.startup_probe_timeout,
            rpc.probe(config.mode == Mode::Transaction),
        )
        .await
        .context("startup probe timed out")??;
        if !startup_probe.supports_block_lookup || !startup_probe.supports_transaction_lookup {
            return Err(anyhow::anyhow!(
                "startup probe failed: read_only profile is missing required query capabilities"
            ));
        }
        if config.mode == Mode::Transaction
            && (!startup_probe.supports_transaction_submission
                || !startup_probe.supports_raw_dry_run)
        {
            return Err(anyhow::anyhow!(
                "startup probe failed: transaction profile is missing submission or dry-run capabilities"
            ));
        }

        let node_info = rpc.node_info().await.map_err(anyhow::Error::from)?;
        let chain_info = rpc.chain_info().await.map_err(anyhow::Error::from)?;
        validate_chain_identity(&config, &node_info, &chain_info).map_err(anyhow::Error::from)?;
        if config.mode == Mode::ReadOnly
            && config.allow_read_only_chain_autodetect
            && (config.expected_chain_id.is_none() || config.expected_network.is_none())
        {
            let context = extract_chain_context(&node_info, &chain_info)?;
            warn!(
                chain_id = context.chain_id,
                network = %context.network,
                genesis_hash = %context.genesis_hash,
                "read_only mode is starting with endpoint autodetect instead of configured chain pins"
            );
        }

        Ok(Self {
            watch_permits: Arc::new(Semaphore::new(config.max_concurrent_watch_requests)),
            expensive_permits: Arc::new(Semaphore::new(config.max_inflight_expensive_requests)),
            transaction_probe: Arc::new(RwLock::new((config.mode == Mode::Transaction).then_some(
                CachedProbe {
                    probe: startup_probe.clone(),
                    observed_at: Instant::now(),
                },
            ))),
            prepared_transactions: Arc::new(RwLock::new(HashMap::new())),
            unresolved_submissions: Arc::new(RwLock::new(HashMap::new())),
            config,
            rpc,
            startup_probe,
        })
    }

    pub fn mode(&self) -> Mode {
        self.config.mode
    }

    pub fn startup_probe(&self) -> &EffectiveProbe {
        &self.startup_probe
    }

    pub async fn chain_status(&self) -> Result<ChainStatusOutput, SharedError> {
        let node_info = self.rpc.node_info().await?;
        let chain_info = self.rpc.chain_info().await?;
        let peers = self
            .rpc
            .node_peers()
            .await?
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let sync_status = self.rpc.sync_status().await?;
        let peer_count = peers
            .as_array()
            .map(|items| items.len() as u64)
            .unwrap_or(0);
        Ok(ChainStatusOutput {
            network: extract_network(&node_info)?,
            chain_id: extract_u8(&chain_info, &["chain_id"])?,
            genesis_hash: extract_string(&chain_info, &["genesis_hash"])?,
            head_block_number: extract_u64(&chain_info, &["head", "number"])?,
            head_block_hash: extract_string(&chain_info, &["head", "block_hash"])?,
            head_state_root: extract_optional_string(&chain_info, &["head", "state_root"]),
            now_seconds: extract_u64(&node_info, &["now_seconds"])?,
            peer_count,
            sync_status,
        })
    }

    pub async fn node_health(&self) -> Result<NodeHealthOutput, SharedError> {
        let node_status = match self.rpc.node_info().await {
            Ok(node_status) => node_status,
            Err(error) if is_transport_error(&error) => {
                return Ok(NodeHealthOutput {
                    node_available: false,
                    node_info: Value::Null,
                    sync: None,
                    peers_summary: json!({
                        "count": 0,
                        "peers": [],
                    }),
                    txpool_summary: None,
                    warnings: vec![format!("node.info unavailable: {}", error.message)],
                });
            }
            Err(error) => return Err(error),
        };
        let mut node_available = true;
        let mut warnings = Vec::new();
        let chain_info = match self.rpc.chain_info().await {
            Ok(chain_info) => Some(chain_info),
            Err(error) if is_transport_error(&error) => {
                node_available = false;
                warnings.push(format!("chain.info unavailable: {}", error.message));
                None
            }
            Err(error) => return Err(error),
        };
        let peers = match self.rpc.node_peers().await {
            Ok(peers) => peers,
            Err(error) if is_transport_error(&error) => {
                node_available = false;
                warnings.push(format!("node.peers unavailable: {}", error.message));
                None
            }
            Err(error) => return Err(error),
        };
        let sync = match self.rpc.sync_status().await {
            Ok(sync) => sync,
            Err(error) if is_transport_error(&error) => {
                warnings.push(format!("sync.status unavailable: {}", error.message));
                None
            }
            Err(error) => return Err(error),
        };
        let txpool = match self.rpc.txpool_state().await {
            Ok(txpool) => txpool,
            Err(error) if is_transport_error(&error) => {
                warnings.push(format!("txpool.state unavailable: {}", error.message));
                None
            }
            Err(error) => return Err(error),
        };

        if let Some(chain_info) = chain_info.as_ref() {
            let now_seconds = extract_u64(&node_status, &["now_seconds"])?;
            let head_timestamp = extract_u64(chain_info, &["head", "timestamp"])?;
            let lag_seconds = now_seconds.saturating_sub(head_timestamp);
            if lag_seconds > self.config.warn_head_lag.as_secs() {
                warnings.push(format!(
                    "head lag is {lag_seconds}s, above warning threshold {}s",
                    self.config.warn_head_lag.as_secs()
                ));
            }
        }
        if peers
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| items.is_empty())
            .unwrap_or(true)
        {
            warnings.push("node reports zero connected peers".to_owned());
        }
        Ok(NodeHealthOutput {
            node_available,
            node_info: node_status,
            sync,
            peers_summary: json!({
                "count": peers.as_ref().and_then(Value::as_array).map(|items| items.len()).unwrap_or(0),
                "peers": peers.unwrap_or_else(|| Value::Array(Vec::new())),
            }),
            txpool_summary: txpool,
            warnings,
        })
    }

    pub async fn get_block(&self, input: GetBlockInput) -> Result<GetBlockOutput, SharedError> {
        let block = self
            .rpc
            .get_block(
                input.block_hash.as_deref(),
                input.block_number,
                input.decode,
                input.include_raw,
            )
            .await?;
        Ok(GetBlockOutput {
            block,
            source: json!({
                "hash": input.block_hash,
                "number": input.block_number,
            }),
        })
    }

    pub async fn list_blocks(
        &self,
        input: ListBlocksInput,
    ) -> Result<ListBlocksOutput, SharedError> {
        let effective_count = input.count.min(self.config.max_list_blocks_count);
        let blocks = self
            .rpc
            .list_blocks(input.from_block_number, effective_count, input.reverse)
            .await?;
        Ok(ListBlocksOutput {
            blocks,
            effective_count,
        })
    }

    pub async fn get_transaction(
        &self,
        input: GetTransactionInput,
    ) -> Result<GetTransactionOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_transaction_lookup,
            "transaction lookup is unavailable for this endpoint profile",
        )?;
        let transaction = self
            .rpc
            .get_transaction(&input.txn_hash, input.decode)
            .await?;
        let transaction_info = if self.startup_probe.supports_transaction_info_lookup {
            self.rpc.get_transaction_info(&input.txn_hash).await?
        } else {
            None
        };
        let events = if input.include_events
            && self.startup_probe.supports_transaction_events_by_hash
            && transaction_info.is_some()
        {
            self.rpc
                .get_events_by_txn_hash(&input.txn_hash, input.decode)
                .await?
        } else {
            Vec::new()
        };
        let status_summary =
            status_summary_from_parts(transaction.as_ref(), transaction_info.as_ref());
        Ok(GetTransactionOutput {
            transaction,
            transaction_info,
            events,
            status_summary,
        })
    }

    pub async fn watch_transaction(
        &self,
        input: WatchTransactionInput,
    ) -> Result<WatchTransactionOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_transaction_lookup
                && self.startup_probe.supports_transaction_info_lookup,
            "watch_transaction is unavailable for this endpoint profile",
        )?;
        let _permit = self.try_watch_permit()?;
        let effective_timeout_seconds = input
            .timeout_seconds
            .unwrap_or(self.config.watch_timeout.as_secs())
            .min(self.config.max_watch_timeout.as_secs());
        let effective_poll_interval_seconds = input
            .poll_interval_seconds
            .unwrap_or(self.config.watch_poll_interval.as_secs())
            .max(self.config.min_watch_poll_interval.as_secs());
        let deadline =
            OffsetDateTime::now_utc().unix_timestamp() as u64 + effective_timeout_seconds;
        let mut last_status_summary = TransactionStatusSummary {
            found: false,
            confirmed: false,
            vm_status: None,
            gas_used: None,
        };
        let mut last_transaction_info = None;
        let mut last_events = Vec::new();
        loop {
            let current = self
                .get_transaction(GetTransactionInput {
                    txn_hash: input.txn_hash.clone(),
                    include_events: true,
                    decode: true,
                })
                .await?;
            if current.status_summary.found {
                self.clear_unresolved_submission(&input.txn_hash).await;
                last_status_summary = current.status_summary.clone();
                last_transaction_info = current.transaction_info.clone();
                last_events = current.events.clone();
            }
            if is_terminal_watch_status(&current.status_summary) {
                return Ok(WatchTransactionOutput {
                    txn_hash: input.txn_hash,
                    found: current.status_summary.found,
                    confirmed: true,
                    effective_timeout_seconds,
                    effective_poll_interval_seconds,
                    transaction_info: current.transaction_info,
                    events: current.events,
                    status_summary: current.status_summary,
                });
            }
            if OffsetDateTime::now_utc().unix_timestamp() as u64 >= deadline {
                return Ok(WatchTransactionOutput {
                    txn_hash: input.txn_hash,
                    found: last_status_summary.found,
                    confirmed: false,
                    effective_timeout_seconds,
                    effective_poll_interval_seconds,
                    transaction_info: last_transaction_info,
                    events: last_events,
                    status_summary: last_status_summary,
                });
            }
            sleep(std::time::Duration::from_secs(
                effective_poll_interval_seconds,
            ))
            .await;
        }
    }

    pub async fn get_events(&self, input: GetEventsInput) -> Result<GetEventsOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_events_query,
            "event query tools are unavailable for this endpoint profile",
        )?;
        let _permit = self.try_expensive_permit()?;
        let effective_limit = input.limit.min(self.config.max_events_limit);
        let events = self
            .rpc
            .get_events(
                input.from_block,
                input.to_block,
                &input.event_keys,
                &input.addresses,
                &input.type_tags,
                effective_limit,
                input.decode,
            )
            .await?;
        let matched_count = events.len() as u64;
        Ok(GetEventsOutput {
            events,
            matched_count,
            effective_limit,
        })
    }

    pub async fn get_account_overview(
        &self,
        input: GetAccountOverviewInput,
    ) -> Result<GetAccountOverviewOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_account_state_lookup,
            "account overview is unavailable for this endpoint profile",
        )?;
        let state = self.rpc.get_account_state(&input.address).await?;
        let onchain_exists = state.is_some();
        let sequence_number = state
            .as_ref()
            .and_then(|value| extract_optional_u64(value, &["sequence_number"]));
        let next_sequence_number_hint = self
            .load_optional_next_sequence_number_hint(&input.address)
            .await?;
        let mut resources = None;
        let mut modules = None;
        let mut balances = Vec::new();
        let mut accepted_tokens = Vec::new();
        let mut applied_resource_limit = None;
        let mut applied_module_limit = None;

        if input.include_resources {
            ensure_capability(
                self.startup_probe.supports_resource_listing,
                "resource listing is unavailable for this endpoint profile",
            )?;
            let _permit = self.try_expensive_permit()?;
            let effective_limit = input
                .resource_limit
                .unwrap_or(self.config.max_account_resource_limit)
                .min(self.config.max_account_resource_limit);
            let listed = self
                .rpc
                .list_resources(&input.address, true, 0, effective_limit, None, &[])
                .await?;
            let mapped = map_named_entries(&listed, "resources");
            extract_balances_and_tokens(&mapped, &mut balances, &mut accepted_tokens);
            resources = Some(mapped);
            applied_resource_limit = Some(effective_limit);
        }

        if input.include_modules {
            ensure_capability(
                self.startup_probe.supports_module_listing,
                "module listing is unavailable for this endpoint profile",
            )?;
            let _permit = self.try_expensive_permit()?;
            let effective_limit = input
                .module_limit
                .unwrap_or(self.config.max_account_module_limit)
                .min(self.config.max_account_module_limit);
            let listed = self.rpc.list_code(&input.address, true, None).await?;
            let mut mapped = map_named_entries(&listed, "codes");
            mapped.truncate(effective_limit as usize);
            modules = Some(mapped);
            applied_module_limit = Some(effective_limit);
        }

        Ok(GetAccountOverviewOutput {
            address: input.address,
            onchain_exists,
            sequence_number,
            balances,
            accepted_tokens,
            resources,
            modules,
            applied_resource_limit,
            applied_module_limit,
            next_sequence_number_hint,
        })
    }

    pub async fn list_resources(
        &self,
        input: ListResourcesInput,
    ) -> Result<ListResourcesOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_resource_listing,
            "resource listing is unavailable for this endpoint profile",
        )?;
        let _permit = self.try_expensive_permit()?;
        let effective_max_size = input
            .max_size
            .unwrap_or(self.config.max_list_resources_size)
            .min(self.config.max_list_resources_size);
        let state_root = self.resolve_state_root(input.block_number).await?;
        let resource_types = input.resource_type.iter().cloned().collect::<Vec<_>>();
        let listed = self
            .rpc
            .list_resources(
                &input.address,
                input.decode,
                input.start_index.unwrap_or(0),
                effective_max_size,
                state_root.clone(),
                &resource_types,
            )
            .await?;
        Ok(ListResourcesOutput {
            address: input.address,
            state_root,
            resources: map_named_entries(&listed, "resources"),
            effective_max_size,
        })
    }

    pub async fn list_modules(
        &self,
        input: ListModulesInput,
    ) -> Result<ListModulesOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_module_listing,
            "module listing is unavailable for this endpoint profile",
        )?;
        let _permit = self.try_expensive_permit()?;
        let effective_max_size = input
            .max_size
            .unwrap_or(self.config.max_list_modules_size)
            .min(self.config.max_list_modules_size);
        let state_root = self.resolve_state_root(input.block_number).await?;
        let listed = self
            .rpc
            .list_code(&input.address, input.resolve_abi, state_root.clone())
            .await?;
        let mut modules = map_named_entries(&listed, "codes");
        modules.truncate(effective_max_size as usize);
        Ok(ListModulesOutput {
            address: input.address,
            state_root,
            modules,
            effective_max_size,
        })
    }

    pub async fn resolve_function_abi(
        &self,
        input: ResolveFunctionAbiInput,
    ) -> Result<Value, SharedError> {
        ensure_capability(
            self.startup_probe.supports_abi_resolution,
            "ABI resolution is unavailable for this endpoint profile",
        )?;
        Ok(json!({ "function_abi": self.rpc.resolve_function_abi(&input.function_id).await? }))
    }

    pub async fn resolve_struct_abi(
        &self,
        input: ResolveStructAbiInput,
    ) -> Result<Value, SharedError> {
        ensure_capability(
            self.startup_probe.supports_abi_resolution,
            "ABI resolution is unavailable for this endpoint profile",
        )?;
        Ok(json!({ "struct_abi": self.rpc.resolve_struct_abi(&input.struct_tag).await? }))
    }

    pub async fn resolve_module_abi(
        &self,
        input: ResolveModuleAbiInput,
    ) -> Result<Value, SharedError> {
        ensure_capability(
            self.startup_probe.supports_abi_resolution,
            "ABI resolution is unavailable for this endpoint profile",
        )?;
        Ok(json!({ "module_abi": self.rpc.resolve_module_abi(&input.module_id).await? }))
    }

    pub async fn call_view_function(
        &self,
        input: CallViewFunctionInput,
    ) -> Result<CallViewFunctionOutput, SharedError> {
        ensure_capability(
            self.startup_probe.supports_view_call,
            "view-function execution is unavailable for this endpoint profile",
        )?;
        let decoded_return_values = self
            .rpc
            .call_view_function(&input.function_id, &input.type_args, &input.args)
            .await?;
        Ok(CallViewFunctionOutput {
            return_values: decoded_return_values.clone(),
            decoded_return_values,
        })
    }

    pub async fn prepare_transfer(
        &self,
        input: PrepareTransferInput,
    ) -> Result<PreparationResult, SharedError> {
        let sender = parse_address(&input.sender)?;
        let receiver = parse_address(&input.receiver)?;
        let amount = input.amount.parse::<u128>().map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidPackagePayload,
                format!("invalid transfer amount: {error}"),
            )
        })?;
        let token_code = parse_token_code(input.token_code.as_deref())?;
        let payload = build_transfer_payload(receiver, amount, token_code.clone())?;
        let summary = json!({
            "kind": "transfer",
            "sender": input.sender,
            "receiver": input.receiver,
            "amount": input.amount,
            "token_code": token_code.to_string(),
        });
        self.prepare_transaction(
            sender,
            input.sender_public_key,
            payload,
            input.sequence_number,
            input.max_gas_amount,
            input.gas_unit_price,
            input.expiration_time_secs,
            input.gas_token,
            TransactionKind::Transfer,
            summary,
        )
        .await
    }

    pub async fn prepare_contract_call(
        &self,
        input: PrepareContractCallInput,
    ) -> Result<PreparationResult, SharedError> {
        let sender = parse_address(&input.sender)?;
        let payload =
            build_contract_call_payload(&input.function_id, &input.type_args, &input.args)?;
        let summary = json!({
            "kind": "contract_call",
            "sender": input.sender,
            "function_id": input.function_id,
            "type_args": input.type_args,
            "args": input.args,
        });
        self.prepare_transaction(
            sender,
            input.sender_public_key,
            payload,
            input.sequence_number,
            input.max_gas_amount,
            input.gas_unit_price,
            input.expiration_time_secs,
            input.gas_token,
            TransactionKind::ContractCall,
            summary,
        )
        .await
    }

    pub async fn prepare_publish_package(
        &self,
        input: PreparePublishPackageInput,
    ) -> Result<PreparationResult, SharedError> {
        let _permit = self.try_expensive_permit()?;
        let payload_len = input.package_bcs_hex.trim_start_matches("0x").len() / 2;
        if payload_len as u64 > self.config.max_publish_package_bytes {
            return Err(SharedError::new(
                SharedErrorCode::PayloadTooLarge,
                format!(
                    "package payload is {payload_len} bytes, above max_publish_package_bytes {}",
                    self.config.max_publish_package_bytes
                ),
            ));
        }
        let sender = parse_address(&input.sender)?;
        let package_bytes = decode_hex_bytes(&input.package_bcs_hex)?;
        let package: Package = bcs_ext::from_bytes(&package_bytes).map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidPackagePayload,
                format!("invalid package bcs payload: {error}"),
            )
        })?;
        let module_count = package.modules().len();
        let payload = TransactionPayload::Package(package);
        let summary = json!({
            "kind": "publish_package",
            "sender": input.sender,
            "module_count": module_count,
            "package_bytes": payload_len,
        });
        self.prepare_transaction(
            sender,
            input.sender_public_key,
            payload,
            input.sequence_number,
            input.max_gas_amount,
            input.gas_unit_price,
            input.expiration_time_secs,
            input.gas_token,
            TransactionKind::PublishPackage,
            summary,
        )
        .await
    }

    pub async fn simulate_raw_transaction(
        &self,
        input: SimulateRawTransactionInput,
    ) -> Result<SimulateRawTransactionOutput, SharedError> {
        self.ensure_transaction_capabilities_current().await?;
        let snapshot = self.load_transaction_endpoint_snapshot().await?;
        let raw_txn_bcs_hex = canonical_hex_payload(&input.raw_txn_bcs_hex)?;
        let simulation = self
            .rpc
            .dry_run_raw(&raw_txn_bcs_hex, &input.sender_public_key)
            .await?;
        let normalized = normalize_simulation(&simulation)?;
        if !normalized.executed {
            return Err(SharedError::new(
                SharedErrorCode::SimulationFailed,
                "dry run returned a failed execution status",
            )
            .with_details(json!({ "simulation": simulation })));
        }
        self.record_prepared_transaction(
            raw_txn_bcs_hex,
            snapshot.chain_context,
            SimulationStatus::Performed,
        )
        .await;
        Ok(SimulateRawTransactionOutput {
            simulation,
            executed: normalized.executed,
            vm_status: normalized.vm_status,
            gas_used: normalized.gas_used,
            events: normalized.events,
            write_set_summary: normalized.write_set_summary,
        })
    }

    pub async fn submit_signed_transaction(
        &self,
        input: SubmitSignedTransactionInput,
    ) -> Result<SubmitSignedTransactionOutput, SharedError> {
        ensure_transaction_mode(self.mode())?;
        let signed_bytes = decode_hex_bytes(&input.signed_txn_bcs_hex)?;
        let signed_txn: SignedUserTransaction =
            bcs_ext::from_bytes(&signed_bytes).map_err(|error| {
                SharedError::new(
                    SharedErrorCode::InvalidPackagePayload,
                    format!("invalid signed transaction bcs hex: {error}"),
                )
            })?;
        let txn_hash = signed_txn.id().to_string();
        let effective_timeout_seconds = effective_submit_timeout_seconds(
            input.blocking,
            input.timeout_seconds,
            self.config.watch_timeout,
            self.config.max_submit_blocking_timeout,
        );
        let raw_txn_bcs_hex = encode_hex_bcs(signed_txn.raw_txn())?;
        let prepared_record = self
            .load_prepared_transaction_record(&raw_txn_bcs_hex)
            .await;
        let prepared_simulation_status = prepared_record
            .as_ref()
            .map(|record| record.simulation_status);
        if self.has_unresolved_submission(&txn_hash).await {
            return Ok(submission_unknown_output(
                txn_hash,
                prepared_simulation_status,
                effective_timeout_seconds,
            ));
        }
        self.ensure_transaction_capabilities_current().await?;
        let snapshot = self.load_transaction_endpoint_snapshot().await?;
        validate_prepared_transaction_record(
            prepared_record.as_ref(),
            &input.prepared_chain_context,
            &snapshot.chain_context,
        )?;
        enforce_submit_simulation_policy(
            self.config.allow_submit_without_prior_simulation,
            prepared_record.as_ref(),
        )?;
        validate_signed_transaction_submission(
            &signed_txn,
            &input.prepared_chain_context,
            &snapshot.chain_context,
        )?;
        if signed_txn.expiration_timestamp_secs() <= snapshot.now_seconds {
            return Ok(rejected_submission_output(
                txn_hash,
                prepared_simulation_status,
                "transaction_expired",
                effective_timeout_seconds,
            ));
        }
        if self
            .signed_transaction_sequence_is_stale(&signed_txn)
            .await?
        {
            return Ok(rejected_submission_output(
                txn_hash,
                prepared_simulation_status,
                "sequence_number_stale",
                effective_timeout_seconds,
            ));
        }

        match self
            .rpc
            .submit_signed_transaction(&input.signed_txn_bcs_hex)
            .await
        {
            Ok(_) => {
                let watch_result = if input.blocking {
                    Some(
                        self.watch_transaction(WatchTransactionInput {
                            txn_hash: txn_hash.clone(),
                            timeout_seconds: effective_timeout_seconds,
                            poll_interval_seconds: Some(self.config.watch_poll_interval.as_secs()),
                        })
                        .await?,
                    )
                } else {
                    None
                };
                Ok(accepted_submission_output(
                    txn_hash,
                    true,
                    prepared_simulation_status,
                    effective_timeout_seconds,
                    watch_result,
                ))
            }
            Err(error)
                if matches!(
                    error.code,
                    SharedErrorCode::NodeUnavailable | SharedErrorCode::RpcUnavailable
                ) =>
            {
                self.record_unresolved_submission(&txn_hash).await;
                Ok(submission_unknown_output(
                    txn_hash,
                    prepared_simulation_status,
                    effective_timeout_seconds,
                ))
            }
            Err(error)
                if matches!(
                    error.code,
                    SharedErrorCode::TransactionExpired | SharedErrorCode::SequenceNumberStale
                ) =>
            {
                Ok(rejected_submission_output(
                    txn_hash,
                    prepared_simulation_status,
                    shared_error_code_name(error.code),
                    effective_timeout_seconds,
                ))
            }
            Err(error) => Err(error),
        }
    }

    async fn prepare_transaction(
        &self,
        sender: AccountAddress,
        sender_public_key: Option<String>,
        payload: TransactionPayload,
        sequence_number: Option<u64>,
        max_gas_amount: Option<u64>,
        gas_unit_price: Option<u64>,
        expiration_time_secs: Option<u64>,
        gas_token: Option<String>,
        transaction_kind: TransactionKind,
        transaction_summary: Value,
    ) -> Result<PreparationResult, SharedError> {
        ensure_transaction_mode(self.mode())?;
        self.ensure_transaction_capabilities_current().await?;
        let snapshot = self.load_transaction_endpoint_snapshot().await?;
        let (sequence_number, sequence_number_source) = self
            .resolve_sequence_number(&sender.to_string(), sequence_number)
            .await?;
        let (gas_unit_price, gas_unit_price_source) =
            self.resolve_gas_price(gas_unit_price).await?;
        let expiration_timestamp_secs =
            self.resolve_expiration(snapshot.now_seconds, expiration_time_secs)?;
        let raw_txn = build_raw_transaction(
            sender,
            sequence_number,
            payload,
            max_gas_amount.unwrap_or(10_000_000),
            gas_unit_price,
            expiration_timestamp_secs,
            gas_token.unwrap_or_else(|| G_STC_TOKEN_CODE.to_string()),
            ChainId::new(snapshot.chain_context.chain_id),
        );
        let raw_txn_bcs_hex = encode_hex_bcs(&raw_txn)?;
        let raw_txn_view = serde_json::to_value(TransactionRequest::from(raw_txn.clone()))
            .map_err(|error| {
                SharedError::new(
                    SharedErrorCode::RpcUnavailable,
                    format!("failed to serialize raw transaction view: {error}"),
                )
            })?;

        let prepared_at = rfc3339_now()?;
        let (simulation_status, simulation, next_action) = match sender_public_key {
            Some(public_key) => {
                let dry_run = self.rpc.dry_run_raw(&raw_txn_bcs_hex, &public_key).await?;
                let normalized = normalize_simulation(&dry_run)?;
                if !normalized.executed {
                    return Err(SharedError::new(
                        SharedErrorCode::SimulationFailed,
                        "dry run returned a failed execution status",
                    )
                    .with_details(json!({ "simulation": dry_run })));
                }
                (
                    SimulationStatus::Performed,
                    Some(normalized),
                    NextAction::SignTransaction,
                )
            }
            None => (
                SimulationStatus::SkippedMissingPublicKey,
                None,
                NextAction::GetPublicKeyThenSimulateOrSign,
            ),
        };
        self.record_prepared_transaction(
            raw_txn_bcs_hex.clone(),
            snapshot.chain_context.clone(),
            simulation_status,
        )
        .await;

        Ok(PreparationResult {
            transaction_kind,
            raw_txn_bcs_hex,
            raw_txn: raw_txn_view,
            transaction_summary,
            chain_context: snapshot.chain_context,
            prepared_at,
            sequence_number_source,
            gas_unit_price_source,
            simulation_status,
            simulation,
            next_action,
        })
    }

    async fn resolve_sequence_number(
        &self,
        address: &str,
        caller_sequence_number: Option<u64>,
    ) -> Result<(u64, SequenceNumberSource), SharedError> {
        if let Some(sequence_number) = caller_sequence_number {
            return Ok((sequence_number, SequenceNumberSource::Caller));
        }
        let txpool_next = self.rpc.next_sequence_number(address).await?;
        let onchain_state = self.rpc.get_account_state(address).await?;
        let onchain_sequence = onchain_state
            .as_ref()
            .and_then(|value| extract_optional_u64(value, &["sequence_number"]));
        match (onchain_sequence, txpool_next) {
            (Some(onchain), Some(txpool)) => Ok((
                onchain.max(txpool),
                SequenceNumberSource::MaxOfOnchainAndTxpool,
            )),
            (Some(onchain), None) => Ok((onchain, SequenceNumberSource::Onchain)),
            (None, Some(txpool)) => Ok((txpool, SequenceNumberSource::Txpool)),
            (None, None) => Err(SharedError::new(
                SharedErrorCode::MissingSender,
                format!("unable to derive sequence number for account {address}"),
            )),
        }
    }

    async fn resolve_gas_price(
        &self,
        caller_gas_price: Option<u64>,
    ) -> Result<(u64, GasUnitPriceSource), SharedError> {
        if let Some(gas_price) = caller_gas_price {
            return Ok((gas_price, GasUnitPriceSource::Caller));
        }
        match self.rpc.gas_price().await {
            Ok(gas_price) => Ok((gas_price, GasUnitPriceSource::Txpool)),
            Err(_) => Ok((1, GasUnitPriceSource::DefaultConfig)),
        }
    }

    fn resolve_expiration(
        &self,
        now_seconds: u64,
        requested_expiration: Option<u64>,
    ) -> Result<u64, SharedError> {
        let max_expiration = now_seconds + self.config.max_expiration_ttl.as_secs();
        let expiration = match requested_expiration {
            Some(value) => value.min(max_expiration),
            None => now_seconds + self.config.default_expiration_ttl.as_secs(),
        };
        Ok(expiration)
    }

    async fn resolve_state_root(
        &self,
        block_number: Option<u64>,
    ) -> Result<Option<String>, SharedError> {
        match block_number {
            Some(block_number) => {
                let block = self
                    .rpc
                    .get_block(None, Some(block_number), true, false)
                    .await?
                    .ok_or_else(|| {
                        SharedError::new(
                            SharedErrorCode::TransactionNotFound,
                            format!("block {block_number} was not found"),
                        )
                    })?;
                Ok(extract_optional_string(&block, &["header", "state_root"]))
            }
            None => Ok(None),
        }
    }

    async fn ensure_transaction_capabilities_current(&self) -> Result<(), SharedError> {
        if self.mode() != Mode::Transaction {
            return Ok(());
        }
        if let Some(cached) = self.transaction_probe.read().await.as_ref() {
            if cached.observed_at.elapsed() <= self.config.chain_status_cache_ttl {
                return validate_transaction_probe(&cached.probe);
            }
        }
        let probe = timeout(self.config.startup_probe_timeout, self.rpc.probe(true))
            .await
            .map_err(|_| {
                SharedError::retryable(
                    SharedErrorCode::NodeUnavailable,
                    "transaction capability probe timed out",
                )
            })??;
        {
            let mut cached = self.transaction_probe.write().await;
            *cached = Some(CachedProbe {
                probe: probe.clone(),
                observed_at: Instant::now(),
            });
        }
        validate_transaction_probe(&probe)
    }

    async fn load_transaction_endpoint_snapshot(
        &self,
    ) -> Result<TransactionEndpointSnapshot, SharedError> {
        let node_info = self.rpc.node_info().await?;
        let chain_info = self.rpc.chain_info_uncached().await?;
        validate_chain_identity(&self.config, &node_info, &chain_info)?;
        let now_seconds = extract_u64(&node_info, &["now_seconds"])?;
        let head_timestamp = extract_u64(&chain_info, &["head", "timestamp"])?;
        enforce_transaction_head_lag(now_seconds, head_timestamp, self.config.max_head_lag)?;
        let chain_context = extract_chain_context(&node_info, &chain_info)?;
        Ok(TransactionEndpointSnapshot {
            chain_context,
            now_seconds,
        })
    }

    async fn signed_transaction_sequence_is_stale(
        &self,
        signed_txn: &SignedUserTransaction,
    ) -> Result<bool, SharedError> {
        let sender = signed_txn.sender().to_string();
        let current_sequence = self.resolve_sequence_number(&sender, None).await?.0;
        Ok(signed_txn.sequence_number() < current_sequence)
    }

    async fn load_optional_next_sequence_number_hint(
        &self,
        address: &str,
    ) -> Result<Option<u64>, SharedError> {
        match self.rpc.next_sequence_number(address).await {
            Ok(sequence_number) => Ok(sequence_number),
            Err(error)
                if matches!(
                    error.code,
                    SharedErrorCode::UnsupportedOperation
                        | SharedErrorCode::NodeUnavailable
                        | SharedErrorCode::RpcUnavailable
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    async fn load_prepared_transaction_record(
        &self,
        raw_txn_bcs_hex: &str,
    ) -> Option<PreparedTransactionRecord> {
        self.prune_prepared_transactions().await;
        self.prepared_transactions
            .read()
            .await
            .get(raw_txn_bcs_hex)
            .cloned()
    }

    async fn record_prepared_transaction(
        &self,
        raw_txn_bcs_hex: String,
        chain_context: ChainContext,
        simulation_status: SimulationStatus,
    ) {
        self.prune_prepared_transactions().await;
        self.prepared_transactions.write().await.insert(
            raw_txn_bcs_hex,
            PreparedTransactionRecord {
                simulation_status,
                chain_context,
                recorded_at: Instant::now(),
            },
        );
    }

    async fn prune_prepared_transactions(&self) {
        let retention = self.config.max_expiration_ttl + self.config.max_watch_timeout;
        let mut prepared = self.prepared_transactions.write().await;
        prepared.retain(|_, record| record.recorded_at.elapsed() <= retention);
    }

    async fn has_unresolved_submission(&self, txn_hash: &str) -> bool {
        self.prune_unresolved_submissions().await;
        self.unresolved_submissions
            .read()
            .await
            .contains_key(txn_hash)
    }

    async fn record_unresolved_submission(&self, txn_hash: &str) {
        self.prune_unresolved_submissions().await;
        self.unresolved_submissions.write().await.insert(
            txn_hash.to_owned(),
            UnresolvedSubmission {
                recorded_at: Instant::now(),
            },
        );
    }

    async fn clear_unresolved_submission(&self, txn_hash: &str) {
        self.unresolved_submissions.write().await.remove(txn_hash);
    }

    async fn prune_unresolved_submissions(&self) {
        let retention = self.config.max_expiration_ttl + self.config.max_watch_timeout;
        let mut unresolved = self.unresolved_submissions.write().await;
        unresolved.retain(|_, submission| submission.recorded_at.elapsed() <= retention);
    }

    fn try_watch_permit(&self) -> Result<OwnedSemaphorePermit, SharedError> {
        self.watch_permits.clone().try_acquire_owned().map_err(|_| {
            SharedError::retryable(
                SharedErrorCode::RateLimited,
                "max_concurrent_watch_requests is exhausted",
            )
        })
    }

    fn try_expensive_permit(&self) -> Result<OwnedSemaphorePermit, SharedError> {
        self.expensive_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                SharedError::retryable(
                    SharedErrorCode::RateLimited,
                    "max_inflight_expensive_requests is exhausted",
                )
            })
    }
}

fn ensure_transaction_mode(mode: Mode) -> Result<(), SharedError> {
    if mode != Mode::Transaction {
        return Err(SharedError::new(
            SharedErrorCode::PermissionDenied,
            "transaction tools are disabled in read_only mode",
        ));
    }
    Ok(())
}

fn ensure_capability(supported: bool, message: &'static str) -> Result<(), SharedError> {
    if supported {
        Ok(())
    } else {
        Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            message,
        ))
    }
}

fn effective_submit_timeout_seconds(
    blocking: bool,
    requested_timeout_seconds: Option<u64>,
    watch_timeout: Duration,
    max_submit_blocking_timeout: Duration,
) -> Option<u64> {
    blocking.then(|| {
        requested_timeout_seconds
            .unwrap_or(watch_timeout.as_secs())
            .min(max_submit_blocking_timeout.as_secs())
    })
}

fn accepted_submission_output(
    txn_hash: String,
    submitted: bool,
    prepared_simulation_status: Option<SimulationStatus>,
    effective_timeout_seconds: Option<u64>,
    watch_result: Option<WatchTransactionOutput>,
) -> SubmitSignedTransactionOutput {
    SubmitSignedTransactionOutput {
        txn_hash,
        submission_state: SubmissionState::Accepted,
        submitted,
        prepared_simulation_status,
        error_code: None,
        effective_timeout_seconds,
        next_action: SubmissionNextAction::WatchTransaction,
        watch_result,
    }
}

fn submission_unknown_output(
    txn_hash: String,
    prepared_simulation_status: Option<SimulationStatus>,
    effective_timeout_seconds: Option<u64>,
) -> SubmitSignedTransactionOutput {
    SubmitSignedTransactionOutput {
        txn_hash,
        submission_state: SubmissionState::Unknown,
        submitted: false,
        prepared_simulation_status,
        error_code: Some("submission_unknown".to_owned()),
        effective_timeout_seconds,
        next_action: SubmissionNextAction::ReconcileByTxnHash,
        watch_result: None,
    }
}

fn rejected_submission_output(
    txn_hash: String,
    prepared_simulation_status: Option<SimulationStatus>,
    error_code: &'static str,
    effective_timeout_seconds: Option<u64>,
) -> SubmitSignedTransactionOutput {
    SubmitSignedTransactionOutput {
        txn_hash,
        submission_state: SubmissionState::Rejected,
        submitted: false,
        prepared_simulation_status,
        error_code: Some(error_code.to_owned()),
        effective_timeout_seconds,
        next_action: SubmissionNextAction::ReprepareThenResign,
        watch_result: None,
    }
}

fn validate_transaction_probe(probe: &EffectiveProbe) -> Result<(), SharedError> {
    if !probe.supports_transaction_submission || !probe.supports_raw_dry_run {
        return Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            "transaction capability probe failed: required submission or dry-run methods are unavailable",
        ));
    }
    Ok(())
}

fn enforce_transaction_head_lag(
    now_seconds: u64,
    head_timestamp: u64,
    max_head_lag: Duration,
) -> Result<(), SharedError> {
    let lag_seconds = now_seconds.saturating_sub(head_timestamp);
    if lag_seconds > max_head_lag.as_secs() {
        return Err(SharedError::retryable(
            SharedErrorCode::RpcUnavailable,
            format!(
                "endpoint head lag is {lag_seconds}s, above max_head_lag {}s",
                max_head_lag.as_secs()
            ),
        ));
    }
    Ok(())
}

fn validate_signed_transaction_submission(
    signed_txn: &SignedUserTransaction,
    prepared_chain_context: &ChainContext,
    current_chain_context: &ChainContext,
) -> Result<(), SharedError> {
    let signed_chain_id = signed_txn.chain_id().id();
    if signed_chain_id != current_chain_context.chain_id {
        return Err(SharedError::new(
            SharedErrorCode::InvalidChainContext,
            format!(
                "signed transaction chain_id {signed_chain_id} does not match current endpoint chain_id {}",
                current_chain_context.chain_id
            ),
        ));
    }
    if prepared_chain_context.chain_id != current_chain_context.chain_id
        || canonicalize_network_name(&prepared_chain_context.network)
            != canonicalize_network_name(&current_chain_context.network)
        || prepared_chain_context.genesis_hash != current_chain_context.genesis_hash
    {
        return Err(SharedError::new(
            SharedErrorCode::InvalidChainContext,
            "prepared chain_context does not match the current endpoint chain identity",
        ));
    }
    if prepared_chain_context.chain_id != signed_chain_id {
        return Err(SharedError::new(
            SharedErrorCode::InvalidChainContext,
            format!(
                "prepared chain_context chain_id {} does not match signed transaction chain_id {signed_chain_id}",
                prepared_chain_context.chain_id
            ),
        ));
    }
    Ok(())
}

fn validate_prepared_transaction_record(
    record: Option<&PreparedTransactionRecord>,
    prepared_chain_context: &ChainContext,
    current_chain_context: &ChainContext,
) -> Result<(), SharedError> {
    let Some(record) = record else {
        return Ok(());
    };
    if record.chain_context.chain_id != prepared_chain_context.chain_id
        || canonicalize_network_name(&record.chain_context.network)
            != canonicalize_network_name(&prepared_chain_context.network)
        || record.chain_context.genesis_hash != prepared_chain_context.genesis_hash
    {
        return Err(SharedError::new(
            SharedErrorCode::InvalidChainContext,
            "prepared transaction attestation does not match the supplied chain_context",
        ));
    }
    if record.chain_context.chain_id != current_chain_context.chain_id
        || canonicalize_network_name(&record.chain_context.network)
            != canonicalize_network_name(&current_chain_context.network)
        || record.chain_context.genesis_hash != current_chain_context.genesis_hash
    {
        return Err(SharedError::new(
            SharedErrorCode::InvalidChainContext,
            "prepared transaction attestation does not match the current endpoint chain identity",
        ));
    }
    Ok(())
}

fn enforce_submit_simulation_policy(
    allow_submit_without_prior_simulation: bool,
    record: Option<&PreparedTransactionRecord>,
) -> Result<(), SharedError> {
    if allow_submit_without_prior_simulation {
        return Ok(());
    }
    match record {
        Some(record) if record.simulation_status == SimulationStatus::Performed => Ok(()),
        Some(record) => Err(SharedError::new(
            SharedErrorCode::PermissionDenied,
            format!(
                "submit blocked by policy: local preparation record has simulation_status = {}",
                serde_json::to_string(&record.simulation_status)
                    .unwrap_or_else(|_| "\"unknown\"".to_owned())
                    .trim_matches('"')
            ),
        )),
        None => Err(SharedError::new(
            SharedErrorCode::PermissionDenied,
            "submit blocked by policy: no local preparation or simulation record exists for this raw transaction",
        )),
    }
}

fn validate_chain_identity(
    config: &RuntimeConfig,
    node_info: &Value,
    chain_info: &Value,
) -> Result<(), SharedError> {
    if let Some(expected_chain_id) = config.expected_chain_id {
        let actual_chain_id = extract_u8(chain_info, &["chain_id"])?;
        if expected_chain_id != actual_chain_id {
            return Err(SharedError::new(
                SharedErrorCode::InvalidChainContext,
                format!(
                    "configured chain_id {expected_chain_id} does not match endpoint chain_id {actual_chain_id}"
                ),
            ));
        }
    }
    if let Some(expected_network) = &config.expected_network {
        let actual_network = extract_network(node_info)?;
        if !canonicalize_network_name(expected_network)
            .eq_ignore_ascii_case(&canonicalize_network_name(&actual_network))
        {
            return Err(SharedError::new(
                SharedErrorCode::InvalidChainContext,
                format!(
                    "configured network {expected_network} does not match endpoint network {actual_network}"
                ),
            ));
        }
    }
    if config.require_genesis_hash_match {
        if let Some(expected_genesis_hash) = &config.expected_genesis_hash {
            let actual_genesis_hash = extract_string(chain_info, &["genesis_hash"])?;
            if expected_genesis_hash != &actual_genesis_hash {
                return Err(SharedError::new(
                    SharedErrorCode::InvalidChainContext,
                    format!(
                        "configured genesis_hash {expected_genesis_hash} does not match endpoint genesis_hash {actual_genesis_hash}"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn extract_chain_context(
    node_info: &Value,
    chain_info: &Value,
) -> Result<ChainContext, SharedError> {
    Ok(ChainContext {
        chain_id: extract_u8(chain_info, &["chain_id"])?,
        network: extract_network(node_info)?,
        genesis_hash: extract_string(chain_info, &["genesis_hash"])?,
        head_block_hash: extract_string(chain_info, &["head", "block_hash"])?,
        head_block_number: extract_u64(chain_info, &["head", "number"])?,
        head_state_root: extract_optional_string(chain_info, &["head", "state_root"]),
        observed_at: rfc3339_now()?,
    })
}

fn extract_network(node_info: &Value) -> Result<String, SharedError> {
    parse_network_name(lookup(node_info, &["net"])).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            "missing or invalid network field at net",
        )
    })
}

fn extract_string(value: &Value, path: &[&str]) -> Result<String, SharedError> {
    extract_optional_string(value, path).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("missing string field at {}", path.join(".")),
        )
    })
}

fn extract_optional_string(value: &Value, path: &[&str]) -> Option<String> {
    lookup(value, path).and_then(|value| match value {
        Value::String(string) => Some(string.clone()),
        Value::Number(number) => Some(number.to_string()),
        other => Some(other.to_string()),
    })
}

fn extract_u64(value: &Value, path: &[&str]) -> Result<u64, SharedError> {
    extract_optional_u64(value, path).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("missing numeric field at {}", path.join(".")),
        )
    })
}

fn extract_optional_u64(value: &Value, path: &[&str]) -> Option<u64> {
    lookup(value, path).and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    })
}

fn extract_u8(value: &Value, path: &[&str]) -> Result<u8, SharedError> {
    extract_u64(value, path).and_then(|value| {
        u8::try_from(value).map_err(|_| {
            SharedError::new(
                SharedErrorCode::RpcUnavailable,
                format!("numeric field at {} does not fit into u8", path.join(".")),
            )
        })
    })
}

fn lookup<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    Some(cursor)
}

fn parse_network_name(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(name) => Some(canonicalize_network_name(name)),
        Value::Object(object) => {
            if let Some(name) = object.get("Builtin").and_then(Value::as_str) {
                return Some(canonicalize_network_name(name));
            }
            if let Some(custom) = object.get("Custom") {
                if let Some(name) = custom.get("chain_name").and_then(Value::as_str) {
                    return Some(canonicalize_network_name(name));
                }
            }
            None
        }
        _ => None,
    }
}

fn canonicalize_network_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn status_summary_from_parts(
    transaction: Option<&Value>,
    transaction_info: Option<&Value>,
) -> TransactionStatusSummary {
    let found = transaction.is_some() || transaction_info.is_some();
    let confirmed = transaction_info.is_some();
    let vm_status = transaction_info
        .and_then(|info| extract_optional_string(info, &["status"]))
        .or_else(|| transaction.and_then(|txn| extract_optional_string(txn, &["status"])));
    let gas_used = transaction_info.and_then(|info| extract_optional_u64(info, &["gas_used"]));
    TransactionStatusSummary {
        found,
        confirmed,
        vm_status,
        gas_used,
    }
}

fn is_terminal_watch_status(summary: &TransactionStatusSummary) -> bool {
    summary.confirmed
}

fn map_named_entries(container: &Value, field: &str) -> Vec<Value> {
    container
        .get(field)
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_balances_and_tokens(
    resources: &[Value],
    balances: &mut Vec<Value>,
    accepted_tokens: &mut Vec<String>,
) {
    for resource in resources {
        let Some(name) = resource.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.contains("Balance") || name.contains("balance") {
            balances.push(resource.clone());
        }
        if name.contains("Token") || name.contains("token") {
            accepted_tokens.push(name.to_owned());
        }
    }
}

fn parse_address(input: &str) -> Result<AccountAddress, SharedError> {
    AccountAddress::from_str(input).map_err(|error| {
        SharedError::new(
            SharedErrorCode::MissingSender,
            format!("invalid account address {input}: {error}"),
        )
    })
}

fn parse_token_code(input: Option<&str>) -> Result<TokenCode, SharedError> {
    match input {
        Some(value) => TokenCode::from_str(value).map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidPackagePayload,
                format!("invalid token code {value}: {error}"),
            )
        }),
        None => Ok(G_STC_TOKEN_CODE.clone()),
    }
}

fn build_contract_call_payload(
    function_id: &str,
    type_args: &[String],
    args: &[String],
) -> Result<TransactionPayload, SharedError> {
    let function_id = FunctionIdView::from_str(function_id).map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidPackagePayload,
            format!("invalid function id {function_id}: {error}"),
        )
    })?;
    let parsed_type_args: Result<Vec<TypeTag>, _> = type_args
        .iter()
        .map(|arg| TypeTagView::from_str(arg).map(|view| view.0))
        .collect();
    let parsed_type_args = parsed_type_args.map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidPackagePayload,
            format!("invalid type args: {error}"),
        )
    })?;
    let parsed_args: Result<Vec<TransactionArgument>, _> = args
        .iter()
        .map(|arg| TransactionArgumentView::from_str(arg).map(|view| view.0))
        .collect();
    let parsed_args = parsed_args.map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidPackagePayload,
            format!("invalid transaction args: {error}"),
        )
    })?;

    Ok(TransactionPayload::EntryFunction(EntryFunction::new(
        function_id.0.module,
        function_id.0.function,
        parsed_type_args,
        convert_txn_args(&parsed_args),
    )))
}

fn build_transfer_payload(
    receiver: AccountAddress,
    amount: u128,
    token_code: TokenCode,
) -> Result<TransactionPayload, SharedError> {
    let token_type = TypeTag::Struct(Box::new(token_code.try_into().map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidPackagePayload,
            format!("invalid token code type conversion: {error}"),
        )
    })?));
    Ok(TransactionPayload::EntryFunction(EntryFunction::new(
        ModuleId::new(
            core_code_address(),
            Identifier::new("transfer_scripts").map_err(|error| {
                SharedError::new(
                    SharedErrorCode::InvalidPackagePayload,
                    format!("failed to construct transfer module identifier: {error}"),
                )
            })?,
        ),
        Identifier::new("peer_to_peer_v2").map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidPackagePayload,
                format!("failed to construct transfer function identifier: {error}"),
            )
        })?,
        vec![token_type],
        vec![
            bcs_ext::to_bytes(&receiver).map_err(|error| {
                SharedError::new(
                    SharedErrorCode::RpcUnavailable,
                    format!("failed to encode transfer receiver: {error}"),
                )
            })?,
            bcs_ext::to_bytes(&amount).map_err(|error| {
                SharedError::new(
                    SharedErrorCode::RpcUnavailable,
                    format!("failed to encode transfer amount: {error}"),
                )
            })?,
        ],
    )))
}

fn build_raw_transaction(
    sender: AccountAddress,
    sequence_number: u64,
    payload: TransactionPayload,
    max_gas_amount: u64,
    gas_unit_price: u64,
    expiration_timestamp_secs: u64,
    gas_token_code: String,
    chain_id: ChainId,
) -> RawUserTransaction {
    RawUserTransaction::new(
        sender,
        sequence_number,
        payload,
        max_gas_amount,
        gas_unit_price,
        expiration_timestamp_secs,
        chain_id,
        gas_token_code,
    )
}

fn normalize_simulation(simulation: &Value) -> Result<SimulationResult, SharedError> {
    let status_string = extract_optional_string(simulation, &["status"])
        .or_else(|| extract_optional_string(simulation, &["txn_output", "status"]))
        .unwrap_or_else(|| "Unknown".to_owned());
    let executed = status_string.eq_ignore_ascii_case("executed")
        || status_string.eq_ignore_ascii_case("\"executed\"");
    let gas_used = extract_optional_u64(simulation, &["gas_used"])
        .or_else(|| extract_optional_u64(simulation, &["txn_output", "gas_used"]))
        .unwrap_or(0);
    let events = simulation
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let write_set_summary = simulation
        .get("write_set")
        .or_else(|| {
            simulation
                .get("txn_output")
                .and_then(|txn| txn.get("write_set"))
        })
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(SimulationResult {
        executed,
        vm_status: status_string,
        gas_used,
        events,
        write_set_summary,
        raw: simulation.clone(),
    })
}

fn shared_error_code_name(code: SharedErrorCode) -> &'static str {
    match code {
        SharedErrorCode::NodeUnavailable => "node_unavailable",
        SharedErrorCode::RpcUnavailable => "rpc_unavailable",
        SharedErrorCode::InvalidChainContext => "invalid_chain_context",
        SharedErrorCode::SubmissionUnknown => "submission_unknown",
        SharedErrorCode::SimulationFailed => "simulation_failed",
        SharedErrorCode::SubmissionFailed => "submission_failed",
        SharedErrorCode::TransactionExpired => "transaction_expired",
        SharedErrorCode::SequenceNumberStale => "sequence_number_stale",
        SharedErrorCode::PermissionDenied => "permission_denied",
        SharedErrorCode::ApprovalRequired => "approval_required",
        SharedErrorCode::RateLimited => "rate_limited",
        SharedErrorCode::PayloadTooLarge => "payload_too_large",
        SharedErrorCode::UnsupportedOperation => "unsupported_operation",
        SharedErrorCode::MissingSender => "missing_sender",
        SharedErrorCode::MissingPublicKey => "missing_public_key",
        SharedErrorCode::InvalidPackagePayload => "invalid_package_payload",
        SharedErrorCode::TransactionNotFound => "transaction_not_found",
    }
}

fn is_transport_error(error: &SharedError) -> bool {
    matches!(
        error.code,
        SharedErrorCode::NodeUnavailable | SharedErrorCode::RpcUnavailable
    )
}

fn decode_hex_bytes(input: &str) -> Result<Vec<u8>, SharedError> {
    hex::decode(input.trim_start_matches("0x")).map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidPackagePayload,
            format!("invalid hex payload: {error}"),
        )
    })
}

fn canonical_hex_payload(input: &str) -> Result<String, SharedError> {
    decode_hex_bytes(input).map(hex::encode)
}

fn encode_hex_bcs<T: serde::Serialize>(value: &T) -> Result<String, SharedError> {
    let bytes = bcs_ext::to_bytes(value).map_err(|error| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("failed to bcs-encode transaction: {error}"),
        )
    })?;
    Ok(hex::encode(bytes))
}

fn rfc3339_now() -> Result<String, SharedError> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(|error| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("failed to format current timestamp: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AppContext, CachedProbe, SignedUserTransaction, accepted_submission_output,
        effective_submit_timeout_seconds, enforce_transaction_head_lag, extract_chain_context,
        is_terminal_watch_status, status_summary_from_parts, submission_unknown_output,
        validate_chain_identity, validate_signed_transaction_submission,
        validate_transaction_probe,
    };
    use httpmock::{Mock, prelude::*};
    use serde_json::{Value, json};
    use starcoin_node_mcp_rpc::NodeRpcClient;
    use starcoin_node_mcp_types::{
        ChainContext, EffectiveProbe, Mode, RuntimeConfig, SimulationStatus, SubmissionState,
        SubmitSignedTransactionInput, VmProfile,
    };
    use starcoin_vm2_crypto::ed25519::genesis_key_pair;
    use starcoin_vm2_vm_types::{
        account_address::AccountAddress,
        on_chain_resource::ChainId,
        transaction::{Script, TransactionPayload},
    };
    use std::{
        collections::HashMap,
        path::PathBuf,
        str::FromStr,
        sync::Arc,
        time::{Duration, Instant},
    };
    use tokio::sync::{RwLock, Semaphore};
    use url::Url;

    #[test]
    fn status_summary_marks_confirmation_when_info_exists() {
        let summary = status_summary_from_parts(
            Some(&json!({"status": "Pending"})),
            Some(&json!({"status": "Executed", "gas_used": "42"})),
        );
        assert!(summary.found);
        assert!(summary.confirmed);
        assert_eq!(summary.gas_used, Some(42));
    }

    #[test]
    fn extract_chain_context_handles_builtin_network_shape() {
        let context = extract_chain_context(
            &json!({
                "net": { "Builtin": "Barnard" },
                "now_seconds": 100,
            }),
            &json!({
                "chain_id": 251,
                "genesis_hash": "0x1",
                "head": {
                    "block_hash": "0x2",
                    "number": "42",
                    "state_root": "0x3",
                }
            }),
        )
        .expect("builtin network should parse");
        assert_eq!(context.network, "barnard");
        assert_eq!(context.chain_id, 251);
    }

    #[test]
    fn validate_chain_identity_accepts_case_insensitive_network_names() {
        let config = sample_runtime_config();
        validate_chain_identity(
            &RuntimeConfig {
                expected_network: Some("main".to_owned()),
                ..config
            },
            &json!({
                "net": { "Builtin": "Main" },
                "now_seconds": 100,
            }),
            &json!({
                "chain_id": 254,
                "genesis_hash": "0x1",
                "head": {
                    "block_hash": "0x2",
                    "number": "1",
                }
            }),
        )
        .expect("builtin network names should compare case-insensitively");
    }

    #[test]
    fn watch_only_terminates_on_confirmation() {
        assert!(!is_terminal_watch_status(&status_summary_from_parts(
            Some(&json!({"status": "Pending"})),
            None,
        )));
        assert!(is_terminal_watch_status(&status_summary_from_parts(
            Some(&json!({"status": "Pending"})),
            Some(&json!({"status": "Executed"})),
        )));
    }

    #[test]
    fn transaction_probe_requires_submission_and_dry_run() {
        validate_transaction_probe(&sample_probe()).expect("fully capable probe should pass");
        let missing_submit = EffectiveProbe {
            supports_transaction_submission: false,
            ..sample_probe()
        };
        assert_eq!(
            validate_transaction_probe(&missing_submit)
                .expect_err("missing submit capability should fail")
                .code,
            starcoin_node_mcp_types::SharedErrorCode::UnsupportedOperation
        );
    }

    #[test]
    fn submit_timeout_is_clamped_when_blocking() {
        assert_eq!(
            effective_submit_timeout_seconds(
                true,
                Some(90),
                Duration::from_secs(30),
                Duration::from_secs(60),
            ),
            Some(60)
        );
        assert_eq!(
            effective_submit_timeout_seconds(
                false,
                Some(90),
                Duration::from_secs(30),
                Duration::from_secs(60),
            ),
            None
        );
    }

    #[test]
    fn head_lag_policy_fails_closed_above_threshold() {
        enforce_transaction_head_lag(120, 90, Duration::from_secs(60))
            .expect("healthy lag should pass");
        assert_eq!(
            enforce_transaction_head_lag(200, 100, Duration::from_secs(30))
                .expect_err("lag above threshold should fail")
                .code,
            starcoin_node_mcp_types::SharedErrorCode::RpcUnavailable
        );
    }

    #[test]
    fn validate_signed_submission_accepts_matching_chain_context() {
        let signed_txn = sample_signed_transaction(254, 7);
        let prepared = sample_chain_context(254, "main", "0x1");
        let current = sample_chain_context(254, "Main", "0x1");
        validate_signed_transaction_submission(&signed_txn, &prepared, &current)
            .expect("matching signed and prepared chain contexts should pass");
    }

    #[test]
    fn validate_signed_submission_rejects_chain_mismatch() {
        let signed_txn = sample_signed_transaction(254, 7);
        let prepared = sample_chain_context(254, "main", "0x1");
        let current = sample_chain_context(253, "barnard", "0x2");
        let error = validate_signed_transaction_submission(&signed_txn, &prepared, &current)
            .expect_err("mismatched endpoint identity should fail");
        assert_eq!(
            error.code,
            starcoin_node_mcp_types::SharedErrorCode::InvalidChainContext
        );
    }

    #[test]
    fn submit_result_helpers_preserve_policy_shape() {
        let accepted = accepted_submission_output(
            "0x1".to_owned(),
            true,
            Some(SimulationStatus::Performed),
            Some(5),
            None,
        );
        assert_eq!(
            accepted.submission_state,
            starcoin_node_mcp_types::SubmissionState::Accepted
        );
        assert!(accepted.submitted);
        assert_eq!(
            accepted.prepared_simulation_status,
            Some(SimulationStatus::Performed)
        );

        let unknown = submission_unknown_output(
            "0x2".to_owned(),
            Some(SimulationStatus::SkippedMissingPublicKey),
            Some(9),
        );
        assert_eq!(unknown.error_code.as_deref(), Some("submission_unknown"));
        assert_eq!(
            unknown.next_action,
            starcoin_node_mcp_types::SubmissionNextAction::ReconcileByTxnHash
        );
    }

    #[tokio::test]
    async fn unresolved_submission_entries_expire_from_local_policy_cache() {
        let app = sample_app_context();
        assert!(!app.has_unresolved_submission("0xabc").await);

        app.record_unresolved_submission("0xabc").await;
        assert!(app.has_unresolved_submission("0xabc").await);

        {
            let mut unresolved = app.unresolved_submissions.write().await;
            unresolved
                .get_mut("0xabc")
                .expect("entry should exist")
                .recorded_at = Instant::now()
                - (app.config.max_expiration_ttl
                    + app.config.max_watch_timeout
                    + Duration::from_secs(1));
        }

        assert!(!app.has_unresolved_submission("0xabc").await);
    }

    #[tokio::test]
    async fn submit_unknown_blocks_blind_resubmission_before_second_txpool_call() {
        let server = MockServer::start();
        let signed_txn = sample_signed_transaction(254, 0);
        let signed_txn_bcs_hex = format!(
            "0x{}",
            hex::encode(bcs_ext::to_bytes(&signed_txn).expect("sample tx should serialize"))
        );
        mock_json_rpc_result(&server, "node.status", json!(true));
        mock_json_rpc_result(&server, "chain.info", sample_chain_info_value());
        mock_json_rpc_result(&server, "node.info", sample_node_info_value());
        mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
        mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
        mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
        mock_json_rpc_error(
            &server,
            "chain.get_transaction_info2",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "chain.get_transaction_info",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "chain.get_events_by_txn_hash2",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "chain.get_events_by_txn_hash",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(&server, "chain.get_events", -32601, "method not found");
        mock_json_rpc_error(&server, "state.list_resource", -32601, "method not found");
        mock_json_rpc_error(&server, "state.list_code", -32601, "method not found");
        mock_json_rpc_error(
            &server,
            "contract2.resolve_function",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "contract.resolve_function",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "contract2.resolve_module",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "contract.resolve_module",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "contract2.resolve_struct",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "contract.resolve_struct",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(&server, "contract2.call_v2", -32601, "method not found");
        mock_json_rpc_error(&server, "contract.call_v2", -32601, "method not found");
        mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
        mock_json_rpc_result(&server, "txpool.next_sequence_number2", json!("0"));
        server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .body_contains("\"method\":\"txpool.submit_hex_transaction2\"")
                .body_contains("\"params\":[]");
            then.status(200)
                .header("content-type", "application/json")
                .body(
                    json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "error": {
                            "code": -32602,
                            "message": "invalid params",
                        }
                    })
                    .to_string(),
                );
        });
        mock_json_rpc_result(
            &server,
            "contract2.dry_run_raw",
            json!({ "status": "Executed" }),
        );
        mock_json_rpc_result(
            &server,
            "state.get_account_state",
            json!({ "sequence_number": "0" }),
        );
        let submit = server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .body_contains("\"method\":\"txpool.submit_hex_transaction2\"")
                .body_contains(&signed_txn_bcs_hex);
            then.status(503).body("submit unavailable");
        });

        let app = AppContext::bootstrap(sample_runtime_config_with_endpoint(&server.url("/")))
            .await
            .expect("bootstrap should succeed");
        let hits_after_bootstrap = submit.hits();
        let input = SubmitSignedTransactionInput {
            signed_txn_bcs_hex,
            prepared_chain_context: sample_chain_context(254, "main", "0x1"),
            blocking: false,
            timeout_seconds: None,
        };

        let first = app
            .submit_signed_transaction(input.clone())
            .await
            .expect("first submission should return an unknown state");
        assert_eq!(first.submission_state, SubmissionState::Unknown);
        assert_eq!(first.error_code.as_deref(), Some("submission_unknown"));
        assert_eq!(submit.hits(), hits_after_bootstrap + 1);

        let second = app
            .submit_signed_transaction(input)
            .await
            .expect("second submission should be blocked locally");
        assert_eq!(second.submission_state, SubmissionState::Unknown);
        assert_eq!(second.error_code.as_deref(), Some("submission_unknown"));
        assert_eq!(submit.hits(), hits_after_bootstrap + 1);
    }

    fn sample_runtime_config() -> RuntimeConfig {
        sample_runtime_config_with_endpoint("https://example.com")
    }

    fn sample_runtime_config_with_endpoint(endpoint: &str) -> RuntimeConfig {
        RuntimeConfig {
            rpc_endpoint_url: Url::parse(endpoint).expect("valid url"),
            mode: Mode::Transaction,
            vm_profile: VmProfile::Auto,
            expected_chain_id: Some(254),
            expected_network: Some("main".to_owned()),
            expected_genesis_hash: Some("0x1".to_owned()),
            require_genesis_hash_match: true,
            connect_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_secs(10),
            startup_probe_timeout: Duration::from_secs(10),
            rpc_auth_token: None,
            rpc_headers: Vec::new(),
            tls_server_name: None,
            allowed_rpc_hosts: Vec::new(),
            tls_pinned_spki_sha256: Vec::new(),
            allow_insecure_remote_transport: false,
            allow_read_only_chain_autodetect: false,
            default_expiration_ttl: Duration::from_secs(600),
            max_expiration_ttl: Duration::from_secs(3_600),
            watch_poll_interval: Duration::from_secs(3),
            watch_timeout: Duration::from_secs(120),
            max_head_lag: Duration::from_secs(60),
            warn_head_lag: Duration::from_secs(15),
            allow_submit_without_prior_simulation: true,
            chain_status_cache_ttl: Duration::from_secs(3),
            abi_cache_ttl: Duration::from_secs(300),
            module_cache_max_entries: 1_024,
            disable_disk_cache: true,
            max_submit_blocking_timeout: Duration::from_secs(60),
            max_watch_timeout: Duration::from_secs(300),
            min_watch_poll_interval: Duration::from_secs(2),
            max_list_blocks_count: 100,
            max_events_limit: 200,
            max_account_resource_limit: 100,
            max_account_module_limit: 50,
            max_list_resources_size: 100,
            max_list_modules_size: 100,
            max_publish_package_bytes: 524_288,
            max_concurrent_watch_requests: 8,
            max_inflight_expensive_requests: 16,
            config_path: Some(PathBuf::from("/tmp/node-mcp.toml")),
            log_level: "info".to_owned(),
        }
    }

    fn sample_probe() -> EffectiveProbe {
        EffectiveProbe {
            supports_node_info: true,
            supports_chain_info: true,
            supports_block_lookup: true,
            supports_block_listing: true,
            supports_transaction_lookup: true,
            supports_transaction_info_lookup: true,
            supports_transaction_events_by_hash: true,
            supports_account_state_lookup: true,
            supports_events_query: true,
            supports_resource_listing: true,
            supports_module_listing: true,
            supports_abi_resolution: true,
            supports_view_call: true,
            supports_transaction_submission: true,
            supports_raw_dry_run: true,
        }
    }

    fn sample_chain_context(chain_id: u8, network: &str, genesis_hash: &str) -> ChainContext {
        ChainContext {
            chain_id,
            network: network.to_owned(),
            genesis_hash: genesis_hash.to_owned(),
            head_block_hash: "0x2".to_owned(),
            head_block_number: 42,
            head_state_root: Some("0x3".to_owned()),
            observed_at: "2026-03-25T00:00:00Z".to_owned(),
        }
    }

    fn sample_signed_transaction(chain_id: u8, sequence_number: u64) -> SignedUserTransaction {
        let sender = AccountAddress::from_str("0x1").expect("valid sender");
        let raw_txn = super::build_raw_transaction(
            sender,
            sequence_number,
            TransactionPayload::Script(Script::new(vec![], vec![], vec![])),
            10_000,
            1,
            1_000,
            "0x1::STC::STC".to_owned(),
            ChainId::new(chain_id),
        );
        let (private_key, public_key) = genesis_key_pair();
        raw_txn
            .sign(&private_key, public_key)
            .expect("sample transaction should sign")
            .into_inner()
    }

    fn sample_app_context() -> AppContext {
        let config = sample_runtime_config();
        AppContext {
            rpc: NodeRpcClient::new(&config).expect("rpc client should build"),
            watch_permits: Arc::new(Semaphore::new(config.max_concurrent_watch_requests)),
            expensive_permits: Arc::new(Semaphore::new(config.max_inflight_expensive_requests)),
            startup_probe: sample_probe(),
            transaction_probe: Arc::new(RwLock::new(Some(CachedProbe {
                probe: sample_probe(),
                observed_at: Instant::now(),
            }))),
            prepared_transactions: Arc::new(RwLock::new(HashMap::new())),
            unresolved_submissions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    fn mock_json_rpc_result<'a>(server: &'a MockServer, method: &str, result: Value) -> Mock<'a> {
        server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .body_contains(&format!("\"method\":\"{method}\""));
            then.status(200)
                .header("content-type", "application/json")
                .body(
                    json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": result,
                    })
                    .to_string(),
                );
        })
    }

    fn mock_json_rpc_error<'a>(
        server: &'a MockServer,
        method: &str,
        code: i64,
        message: &str,
    ) -> Mock<'a> {
        server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .body_contains(&format!("\"method\":\"{method}\""));
            then.status(200)
                .header("content-type", "application/json")
                .body(
                    json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "error": {
                            "code": code,
                            "message": message,
                        }
                    })
                    .to_string(),
                );
        })
    }

    fn sample_node_info_value() -> Value {
        json!({
            "net": { "Builtin": "Main" },
            "now_seconds": 120,
        })
    }

    fn sample_chain_info_value() -> Value {
        json!({
            "chain_id": 254,
            "genesis_hash": "0x1",
            "head": {
                "number": 42,
                "block_hash": "0x2",
                "state_root": "0x3",
                "timestamp": 100,
            }
        })
    }
}
