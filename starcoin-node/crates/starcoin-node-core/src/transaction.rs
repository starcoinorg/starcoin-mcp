use super::*;

impl AppContext {
    pub async fn prepare_transfer(
        &self,
        input: PrepareTransferInput,
    ) -> Result<PreparationResult, SharedError> {
        let sender_text = input.sender.clone();
        let receiver_text = input.receiver.clone();
        let amount_text = input.amount.clone();
        let sender = parse_address("sender", &input.sender)?;
        let receiver = parse_address("receiver", &input.receiver)?;
        let amount = input.amount.parse::<u128>().map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidAmount,
                format!("invalid transfer amount: {error}"),
            )
        })?;
        let token_code = parse_token_code(input.token_code.as_deref())?;
        let payload = build_transfer_payload(receiver, amount, token_code.clone())?;
        let summary = json!({
            "kind": "transfer",
            "sender": sender_text,
            "receiver": receiver_text,
            "amount": amount_text,
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
            Some(PreparedTransferDetails {
                receiver: input.receiver,
                amount: input.amount,
                token_code: token_code.to_string(),
            }),
        )
        .await
    }

    pub async fn prepare_contract_call(
        &self,
        input: PrepareContractCallInput,
    ) -> Result<PreparationResult, SharedError> {
        let sender = parse_address("sender", &input.sender)?;
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
            None,
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
        let sender = parse_address("sender", &input.sender)?;
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
            None,
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
        transfer_details: Option<PreparedTransferDetails>,
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
        let max_gas_amount = max_gas_amount.unwrap_or(10_000_000);
        let gas_token_code = normalize_gas_token_code(gas_token.as_deref())?;
        let raw_txn = build_raw_transaction(
            sender,
            sequence_number,
            payload,
            max_gas_amount,
            gas_unit_price,
            expiration_timestamp_secs,
            gas_token_code.clone(),
            ChainId::new(snapshot.chain_context.chain_id),
        );
        let raw_txn_bcs_hex = encode_hex_bcs(&raw_txn)?;
        let raw_txn_view = serde_json::to_value(TransactionRequest::from(raw_txn.clone()))
            .map_err(|error| {
                SharedError::new(
                    SharedErrorCode::InvalidPackagePayload,
                    format!("failed to serialize raw transaction view: {error}"),
                )
            })?;

        let prepared_at = rfc3339_now();
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
        let estimated_network_fee = simulation
            .as_ref()
            .map(|result| multiply_u64_to_string(result.gas_used, gas_unit_price));
        let execution_facts = PreparedExecutionFacts {
            sender: sender.to_string(),
            sequence_number,
            max_gas_amount,
            gas_unit_price,
            gas_token_code,
            expiration_timestamp_secs,
            chain_id: snapshot.chain_context.chain_id,
            estimated_max_network_fee: multiply_u64_to_string(max_gas_amount, gas_unit_price),
            estimated_network_fee,
            transfer_receiver: transfer_details
                .as_ref()
                .map(|details| details.receiver.clone()),
            transfer_amount: transfer_details
                .as_ref()
                .map(|details| details.amount.clone()),
            transfer_token_code: transfer_details
                .as_ref()
                .map(|details| details.token_code.clone()),
        };

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
            execution_facts,
            next_action,
        })
    }

    pub(crate) async fn resolve_sequence_number(
        &self,
        address: &str,
        caller_sequence_number: Option<u64>,
    ) -> Result<(u64, SequenceNumberSource), SharedError> {
        if let Some(sequence_number) = caller_sequence_number {
            return Ok((sequence_number, SequenceNumberSource::Caller));
        }
        let txpool_next = match self.rpc.next_sequence_number(address).await {
            Ok(sequence_number) => sequence_number,
            Err(error) if is_degradable_sequence_lookup_error(&error) => None,
            Err(error) => return Err(error),
        };
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

    pub(crate) fn resolve_expiration(
        &self,
        now_seconds: u64,
        requested_expiration: Option<u64>,
    ) -> Result<u64, SharedError> {
        let max_expiration = now_seconds.saturating_add(self.config.max_expiration_ttl.as_secs());
        let expiration = match requested_expiration {
            Some(value) if value < now_seconds => {
                return Err(SharedError::new(
                    SharedErrorCode::TransactionExpired,
                    format!(
                        "requested expiration {value} is earlier than current node time {now_seconds}"
                    ),
                ));
            }
            Some(value) => value.min(max_expiration),
            None => now_seconds.saturating_add(self.config.default_expiration_ttl.as_secs()),
        };
        Ok(expiration)
    }
}

#[derive(Clone, Debug)]
struct PreparedTransferDetails {
    receiver: String,
    amount: String,
    token_code: String,
}

fn parse_address(field_name: &str, input: &str) -> Result<AccountAddress, SharedError> {
    AccountAddress::from_str(input).map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidAddress,
            format!("invalid {field_name} address {input}: {error}"),
        )
    })
}

fn parse_token_code(input: Option<&str>) -> Result<TokenCode, SharedError> {
    match input {
        Some(value) => TokenCode::from_str(value).map_err(|error| {
            SharedError::new(
                SharedErrorCode::InvalidAsset,
                format!("invalid token code {value}: {error}"),
            )
        }),
        None => Ok(G_STC_TOKEN_CODE.clone()),
    }
}

fn normalize_gas_token_code(input: Option<&str>) -> Result<String, SharedError> {
    match input {
        Some(value) => parse_token_code(Some(value)).map(|token| token.to_string()),
        None => Ok(G_STC_TOKEN_CODE.to_string()),
    }
}

fn multiply_u64_to_string(left: u64, right: u64) -> String {
    (u128::from(left) * u128::from(right)).to_string()
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
                    SharedErrorCode::InvalidPackagePayload,
                    format!("failed to encode transfer receiver: {error}"),
                )
            })?,
            bcs_ext::to_bytes(&amount).map_err(|error| {
                SharedError::new(
                    SharedErrorCode::InvalidPackagePayload,
                    format!("failed to encode transfer amount: {error}"),
                )
            })?,
        ],
    )))
}

pub(crate) fn build_raw_transaction(
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
        .or_else(|| {
            simulation
                .get("txn_output")
                .and_then(|txn| txn.get("events"))
        })
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
