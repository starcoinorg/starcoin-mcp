use super::*;

impl AppContext {
    pub async fn bootstrap(config: RuntimeConfig) -> anyhow::Result<Self> {
        config.validate()?;
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

    pub(crate) async fn ensure_transaction_capabilities_current(&self) -> Result<(), SharedError> {
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

    pub(crate) async fn load_transaction_endpoint_snapshot(
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
}

pub(crate) fn validate_transaction_probe(probe: &EffectiveProbe) -> Result<(), SharedError> {
    if !probe.supports_block_lookup
        || !probe.supports_transaction_lookup
        || !probe.supports_transaction_submission
        || !probe.supports_raw_dry_run
    {
        return Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            "transaction capability probe failed: required read-side, submission, or dry-run methods are unavailable",
        ));
    }
    Ok(())
}

pub(crate) fn enforce_transaction_head_lag(
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
