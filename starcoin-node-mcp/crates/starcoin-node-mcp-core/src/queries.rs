use super::*;

impl AppContext {
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
        if matches!(peers.as_ref().and_then(Value::as_array), Some(items) if items.is_empty()) {
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

    pub(crate) async fn resolve_state_root(
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
                            SharedErrorCode::BlockNotFound,
                            format!("block {block_number} was not found"),
                        )
                    })?;
                let state_root = extract_optional_string(&block, &["header", "state_root"])
                    .ok_or_else(|| {
                        SharedError::new(
                            SharedErrorCode::RpcUnavailable,
                            format!("block {block_number} is missing header.state_root"),
                        )
                    })?;
                Ok(Some(state_root))
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn load_optional_next_sequence_number_hint(
        &self,
        address: &str,
    ) -> Result<Option<u64>, SharedError> {
        match self.rpc.next_sequence_number(address).await {
            Ok(sequence_number) => Ok(sequence_number),
            Err(error) if is_degradable_sequence_lookup_error(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
