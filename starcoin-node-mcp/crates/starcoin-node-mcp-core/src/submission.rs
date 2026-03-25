use super::*;

impl AppContext {
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
                    match self
                        .watch_transaction(WatchTransactionInput {
                            txn_hash: txn_hash.clone(),
                            timeout_seconds: effective_timeout_seconds,
                            poll_interval_seconds: Some(self.config.watch_poll_interval.as_secs()),
                        })
                        .await
                    {
                        Ok(result) => Some(result),
                        Err(error) => {
                            warn!(
                                txn_hash = %txn_hash,
                                error = %error,
                                "blocking watch failed after a successful submit; returning accepted submission result",
                            );
                            None
                        }
                    }
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
                    SharedErrorCode::NodeUnavailable
                        | SharedErrorCode::RpcUnavailable
                        | SharedErrorCode::SubmissionUnknown
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

    async fn signed_transaction_sequence_is_stale(
        &self,
        signed_txn: &SignedUserTransaction,
    ) -> Result<bool, SharedError> {
        let sender = signed_txn.sender().to_string();
        let current_sequence = self.resolve_sequence_number(&sender, None).await?.0;
        Ok(signed_txn.sequence_number() < current_sequence)
    }

    pub(crate) async fn load_prepared_transaction_record(
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

    pub(crate) async fn record_prepared_transaction(
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

    pub(crate) async fn has_unresolved_submission(&self, txn_hash: &str) -> bool {
        self.prune_unresolved_submissions().await;
        self.unresolved_submissions
            .read()
            .await
            .contains_key(txn_hash)
    }

    pub(crate) async fn record_unresolved_submission(&self, txn_hash: &str) {
        self.prune_unresolved_submissions().await;
        self.unresolved_submissions.write().await.insert(
            txn_hash.to_owned(),
            UnresolvedSubmission {
                recorded_at: Instant::now(),
            },
        );
    }

    pub(crate) async fn clear_unresolved_submission(&self, txn_hash: &str) {
        self.unresolved_submissions.write().await.remove(txn_hash);
    }

    async fn prune_unresolved_submissions(&self) {
        let retention = self.config.max_expiration_ttl + self.config.max_watch_timeout;
        let mut unresolved = self.unresolved_submissions.write().await;
        unresolved.retain(|_, submission| submission.recorded_at.elapsed() <= retention);
    }

    pub(crate) fn try_watch_permit(&self) -> Result<OwnedSemaphorePermit, SharedError> {
        self.watch_permits.clone().try_acquire_owned().map_err(|_| {
            SharedError::retryable(
                SharedErrorCode::RateLimited,
                "max_concurrent_watch_requests is exhausted",
            )
        })
    }

    pub(crate) fn try_expensive_permit(&self) -> Result<OwnedSemaphorePermit, SharedError> {
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

pub(crate) fn ensure_transaction_mode(mode: Mode) -> Result<(), SharedError> {
    if mode != Mode::Transaction {
        return Err(SharedError::new(
            SharedErrorCode::PermissionDenied,
            "transaction tools are disabled in read_only mode",
        ));
    }
    Ok(())
}

pub(crate) fn effective_submit_timeout_seconds(
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

pub(crate) fn accepted_submission_output(
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

pub(crate) fn submission_unknown_output(
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

pub(crate) fn validate_signed_transaction_submission(
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
