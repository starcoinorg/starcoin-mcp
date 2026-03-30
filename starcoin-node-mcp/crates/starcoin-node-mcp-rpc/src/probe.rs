use super::*;

impl NodeRpcClient {
    pub async fn probe(&self, mode_transaction: bool) -> Result<EffectiveProbe, SharedError> {
        let _status: bool = self.call("node.status", json!([])).await?;
        let _chain_info = self.chain_info_uncached().await?;
        let _node_info = self.node_info().await?;

        let supports_block_lookup = self
            .probe_method_supported("chain.get_block_by_number", json!([0u64, Value::Null]))
            .await?;
        let supports_block_listing = self.supports_block_listing().await?;
        let supports_transaction_lookup = self.supports_transaction_lookup().await?;
        let supports_transaction_info_lookup = self.supports_transaction_info_lookup().await?;
        let supports_transaction_events_by_hash =
            self.supports_transaction_events_by_hash().await?;
        let supports_account_state_lookup = self.supports_account_state_lookup().await?;
        let supports_events_query = self.supports_events_query().await?;
        let supports_resource_listing = self.supports_resource_listing().await?;
        let supports_module_listing = self.supports_module_listing().await?;
        let supports_abi_resolution = self.supports_abi_resolution().await?;
        let supports_view_call = self.supports_view_call().await?;
        let supports_transaction_submission = if mode_transaction {
            self.supports_submission().await?
        } else {
            false
        };
        let supports_raw_dry_run = if mode_transaction {
            self.supports_raw_dry_run().await?
        } else {
            false
        };

        Ok(EffectiveProbe {
            supports_node_info: true,
            supports_chain_info: true,
            supports_block_lookup,
            supports_block_listing,
            supports_transaction_lookup,
            supports_transaction_info_lookup,
            supports_transaction_events_by_hash,
            supports_account_state_lookup,
            supports_events_query,
            supports_resource_listing,
            supports_module_listing,
            supports_abi_resolution,
            supports_view_call,
            supports_transaction_submission,
            supports_raw_dry_run,
        })
    }

    pub(crate) fn transaction_methods<'a>(
        &self,
        preferred: &'a str,
        fallback: &'a str,
    ) -> Vec<&'a str> {
        match self.vm_profile {
            VmProfile::Vm2Only => vec![preferred],
            VmProfile::LegacyCompatible => vec![fallback, preferred],
            VmProfile::Auto => vec![preferred, fallback],
        }
    }

    async fn probe_method_supported(
        &self,
        method: &str,
        params: Value,
    ) -> Result<bool, SharedError> {
        match self.call_value(method, params).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => Ok(false),
            Err(error) if error.retryable => Err(error),
            Err(error) if is_invalid_params_error(&error) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn supports_block_listing(&self) -> Result<bool, SharedError> {
        self.probe_method_supported(
            "chain.get_blocks_by_number",
            json!([Value::Null, 1u64, {
                "reverse": false,
                "decode": true,
                "raw": false,
            }]),
        )
        .await
    }

    async fn supports_transaction_lookup(&self) -> Result<bool, SharedError> {
        for method in self.transaction_methods("chain.get_transaction2", "chain.get_transaction") {
            if self
                .probe_method_supported(
                    method,
                    json!(["0x0000000000000000000000000000000000000000000000000000000000000000", { "decode": true }]),
                )
                .await?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn supports_transaction_info_lookup(&self) -> Result<bool, SharedError> {
        self.supports_any_method(
            &self.transaction_methods("chain.get_transaction_info2", "chain.get_transaction_info"),
            json!(["0x0000000000000000000000000000000000000000000000000000000000000000"]),
        )
        .await
    }

    async fn supports_transaction_events_by_hash(&self) -> Result<bool, SharedError> {
        self.supports_any_method(
            &self.transaction_methods(
                "chain.get_events_by_txn_hash2",
                "chain.get_events_by_txn_hash",
            ),
            json!(["0x0000000000000000000000000000000000000000000000000000000000000000", {
                "decode": true,
            }]),
        )
        .await
    }

    async fn supports_account_state_lookup(&self) -> Result<bool, SharedError> {
        if self.vm_profile == VmProfile::Vm2Only {
            return self
                .probe_method_supported(
                    "state2.list_resource",
                    vm2_list_resources_params(
                        "0x00000000000000000000000000000000",
                        true,
                        0,
                        1,
                        None,
                        &[],
                    ),
                )
                .await;
        }
        self.probe_method_supported(
            "state.get_account_state",
            json!(["0x00000000000000000000000000000000"]),
        )
        .await
    }

    async fn supports_events_query(&self) -> Result<bool, SharedError> {
        self.probe_method_supported(
            "chain.get_events",
            json!([{}, { "limit": 1u64, "decode": true }]),
        )
        .await
    }

    async fn supports_resource_listing(&self) -> Result<bool, SharedError> {
        if self.vm_profile == VmProfile::Vm2Only {
            return self
                .probe_method_supported(
                    "state2.list_resource",
                    vm2_list_resources_params(
                        "0x00000000000000000000000000000000",
                        true,
                        0,
                        1,
                        None,
                        &[],
                    ),
                )
                .await;
        }
        self.probe_method_supported(
            "state.list_resource",
            json!(["0x00000000000000000000000000000000", {
                "decode": true,
                "start_index": 0u64,
                "max_size": 1u64
            }]),
        )
        .await
    }

    async fn supports_module_listing(&self) -> Result<bool, SharedError> {
        if self.vm_profile == VmProfile::Vm2Only {
            return self
                .probe_method_supported(
                    "state2.list_code",
                    vm2_list_code_params("0x00000000000000000000000000000000", true, None),
                )
                .await;
        }
        self.probe_method_supported(
            "state.list_code",
            json!(["0x00000000000000000000000000000000", {
                "resolve": true
            }]),
        )
        .await
    }

    async fn supports_abi_resolution(&self) -> Result<bool, SharedError> {
        let function = self
            .supports_any_method(
                &self
                    .transaction_methods("contract2.resolve_function", "contract.resolve_function"),
                json!(["0x1::Account::balance"]),
            )
            .await?;
        let module = self
            .supports_any_method(
                &self.transaction_methods("contract2.resolve_module", "contract.resolve_module"),
                json!(["0x1::Account"]),
            )
            .await?;
        let structure = self
            .supports_any_method(
                &self.transaction_methods("contract2.resolve_struct", "contract.resolve_struct"),
                json!(["0x1::Account::Account"]),
            )
            .await?;
        Ok(function && module && structure)
    }

    async fn supports_view_call(&self) -> Result<bool, SharedError> {
        self.supports_any_method(
            &self.transaction_methods("contract2.call_v2", "contract.call_v2"),
            json!([{
                "function_id": "0x1::Account::balance",
                "type_args": [],
                "args": []
            }]),
        )
        .await
    }

    async fn supports_submission(&self) -> Result<bool, SharedError> {
        let gas_price = self
            .probe_method_supported("txpool.gas_price", json!([]))
            .await?;
        let sequence = self
            .probe_method_supported(
                "txpool.next_sequence_number2",
                json!(["0x00000000000000000000000000000000"]),
            )
            .await?
            || self
                .probe_method_supported(
                    "txpool.next_sequence_number",
                    json!(["0x00000000000000000000000000000000"]),
                )
                .await?;
        let submit = self
            .probe_method_supported("txpool.submit_hex_transaction2", json!([]))
            .await?
            || self
                .probe_method_supported("txpool.submit_hex_transaction", json!([]))
                .await?;
        Ok(gas_price && sequence && submit)
    }

    async fn supports_raw_dry_run(&self) -> Result<bool, SharedError> {
        for method in self.transaction_methods("contract2.dry_run_raw", "contract.dry_run_raw") {
            if self
                .probe_method_supported(method, json!(["0x00", "0x00"]))
                .await?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn supports_any_method(
        &self,
        methods: &[&str],
        params: Value,
    ) -> Result<bool, SharedError> {
        for method in methods {
            if self.probe_method_supported(method, params.clone()).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn is_invalid_params_error(error: &SharedError) -> bool {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("rpc_code"))
        .and_then(Value::as_i64)
        == Some(-32602)
}
