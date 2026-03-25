#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::Context;
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use starcoin_node_mcp_types::{
    EffectiveProbe, RuntimeConfig, SharedError, SharedErrorCode, VmProfile,
};
use tokio::sync::RwLock;
use url::Url;

#[derive(Debug)]
pub struct NodeRpcClient {
    endpoint: Url,
    http: Client,
    next_id: AtomicU64,
    vm_profile: VmProfile,
    chain_info_cache: TimedValueCache,
    abi_cache: TimedValueCache,
}

impl NodeRpcClient {
    pub fn new(config: &RuntimeConfig) -> anyhow::Result<Self> {
        let mut default_headers = HeaderMap::new();
        if let Some(token) = &config.rpc_auth_token {
            let header_value = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
                .context("invalid rpc auth token header value")?;
            default_headers.insert(AUTHORIZATION, header_value);
        }
        for (name, value) in &config.rpc_headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid rpc header name {name}"))?;
            let value = HeaderValue::from_str(value.expose())
                .with_context(|| format!("invalid rpc header value for {name}"))?;
            default_headers.insert(name, value);
        }
        let http = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .default_headers(default_headers)
            .build()
            .context("failed to build rpc http client")?;

        Ok(Self {
            endpoint: config.rpc_endpoint_url.clone(),
            http,
            next_id: AtomicU64::new(1),
            vm_profile: config.vm_profile,
            chain_info_cache: TimedValueCache::new(config.chain_status_cache_ttl, 8),
            abi_cache: TimedValueCache::new(
                config.abi_cache_ttl,
                config.module_cache_max_entries as usize,
            ),
        })
    }

    pub async fn probe(&self, mode_transaction: bool) -> Result<EffectiveProbe, SharedError> {
        let _status: bool = self.call("node.status", json!([])).await?;
        let _chain_info = self.chain_info().await?;
        let _node_info = self.node_info().await?;

        let supports_block_lookup = self
            .method_exists("chain.get_block_by_number", json!([0u64, Value::Null]))
            .await?;
        let supports_transaction_lookup = self.supports_transaction_lookup().await?;
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
            supports_transaction_lookup,
            supports_transaction_submission,
            supports_raw_dry_run,
        })
    }

    pub async fn node_info(&self) -> Result<Value, SharedError> {
        self.call_value("node.info", json!([])).await
    }

    pub async fn node_peers(&self) -> Result<Option<Value>, SharedError> {
        self.optional_call_value("node.peers", json!([])).await
    }

    pub async fn sync_status(&self) -> Result<Option<Value>, SharedError> {
        self.optional_call_value("sync.status", json!([])).await
    }

    pub async fn txpool_state(&self) -> Result<Option<Value>, SharedError> {
        self.optional_call_value("txpool.state", json!([])).await
    }

    pub async fn chain_info(&self) -> Result<Value, SharedError> {
        self.cached_value(&self.chain_info_cache, "chain.info", || async {
            self.call_value("chain.info", json!([])).await
        })
        .await
    }

    pub async fn get_block(
        &self,
        block_hash: Option<&str>,
        block_number: Option<u64>,
        decode: bool,
        include_raw: bool,
    ) -> Result<Option<Value>, SharedError> {
        let option = json!({
            "decode": decode,
            "raw": include_raw,
        });
        match (block_hash, block_number) {
            (Some(hash), None) => {
                self.call("chain.get_block_by_hash", json!([hash, option]))
                    .await
            }
            (None, Some(number)) => {
                self.call("chain.get_block_by_number", json!([number, option]))
                    .await
            }
            _ => Err(SharedError::new(
                SharedErrorCode::UnsupportedOperation,
                "get_block requires exactly one of block_hash or block_number",
            )),
        }
    }

    pub async fn list_blocks(
        &self,
        from_block_number: Option<u64>,
        count: u64,
        reverse: bool,
    ) -> Result<Vec<Value>, SharedError> {
        let option = json!({
            "reverse": reverse,
            "decode": true,
            "raw": false,
        });
        self.call(
            "chain.get_blocks_by_number",
            json!([from_block_number, count, option]),
        )
        .await
    }

    pub async fn get_transaction(
        &self,
        txn_hash: &str,
        decode: bool,
    ) -> Result<Option<Value>, SharedError> {
        self.call_first_available(
            &self.transaction_methods("chain.get_transaction2", "chain.get_transaction"),
            json!([txn_hash, { "decode": decode }]),
        )
        .await
    }

    pub async fn get_transaction_info(&self, txn_hash: &str) -> Result<Option<Value>, SharedError> {
        self.call_first_available(
            &self.transaction_methods("chain.get_transaction_info2", "chain.get_transaction_info"),
            json!([txn_hash]),
        )
        .await
    }

    pub async fn get_events_by_txn_hash(
        &self,
        txn_hash: &str,
        decode: bool,
    ) -> Result<Vec<Value>, SharedError> {
        self.call_first_available_vec(
            &self.transaction_methods(
                "chain.get_events_by_txn_hash2",
                "chain.get_events_by_txn_hash",
            ),
            json!([txn_hash, { "decode": decode }]),
        )
        .await
    }

    pub async fn get_events(
        &self,
        from_block: Option<u64>,
        to_block: Option<u64>,
        event_keys: &[String],
        addresses: &[String],
        type_tags: &[String],
        limit: u64,
        decode: bool,
    ) -> Result<Vec<Value>, SharedError> {
        let filter = json!({
            "from_block": from_block,
            "to_block": to_block,
            "event_keys": event_keys,
            "addrs": addresses,
            "type_tags": type_tags,
        });
        self.call(
            "chain.get_events",
            json!([filter, { "limit": limit, "decode": decode }]),
        )
        .await
    }

    pub async fn get_account_state(&self, address: &str) -> Result<Option<Value>, SharedError> {
        self.call("state.get_account_state", json!([address])).await
    }

    pub async fn get_state_root(&self) -> Result<Value, SharedError> {
        self.call_value("state.get_state_root", json!([])).await
    }

    pub async fn list_resources(
        &self,
        address: &str,
        decode: bool,
        start_index: u64,
        max_size: u64,
        state_root: Option<String>,
        resource_types: &[String],
    ) -> Result<Value, SharedError> {
        self.call_value(
            "state.list_resource",
            json!([
                address,
                {
                    "decode": decode,
                    "state_root": state_root,
                    "start_index": start_index,
                    "max_size": max_size,
                    "resource_types": if resource_types.is_empty() { Value::Null } else { json!(resource_types) }
                }
            ]),
        )
        .await
    }

    pub async fn list_code(
        &self,
        address: &str,
        resolve: bool,
        state_root: Option<String>,
    ) -> Result<Value, SharedError> {
        self.call_value(
            "state.list_code",
            json!([
                address,
                {
                    "resolve": resolve,
                    "state_root": state_root,
                }
            ]),
        )
        .await
    }

    pub async fn resolve_function_abi(&self, function_id: &str) -> Result<Value, SharedError> {
        let key = format!("function:{function_id}");
        self.cached_value(&self.abi_cache, &key, || async {
            self.call_first_available_value(
                &self
                    .transaction_methods("contract2.resolve_function", "contract.resolve_function"),
                json!([function_id]),
            )
            .await
        })
        .await
    }

    pub async fn resolve_struct_abi(&self, struct_tag: &str) -> Result<Value, SharedError> {
        let key = format!("struct:{struct_tag}");
        self.cached_value(&self.abi_cache, &key, || async {
            self.call_first_available_value(
                &self.transaction_methods("contract2.resolve_struct", "contract.resolve_struct"),
                json!([struct_tag]),
            )
            .await
        })
        .await
    }

    pub async fn resolve_module_abi(&self, module_id: &str) -> Result<Value, SharedError> {
        let key = format!("module:{module_id}");
        self.cached_value(&self.abi_cache, &key, || async {
            self.call_first_available_value(
                &self.transaction_methods("contract2.resolve_module", "contract.resolve_module"),
                json!([module_id]),
            )
            .await
        })
        .await
    }

    pub async fn call_view_function(
        &self,
        function_id: &str,
        type_args: &[String],
        args: &[String],
    ) -> Result<Vec<Value>, SharedError> {
        self.call_first_available_vec(
            &self.transaction_methods("contract2.call_v2", "contract.call_v2"),
            json!([{
                "function_id": function_id,
                "type_args": type_args,
                "args": args,
            }]),
        )
        .await
    }

    pub async fn gas_price(&self) -> Result<u64, SharedError> {
        let value: Value = self.call_value("txpool.gas_price", json!([])).await?;
        parse_u64(&value).ok_or_else(|| {
            SharedError::new(
                SharedErrorCode::RpcUnavailable,
                "txpool.gas_price returned an invalid value",
            )
        })
    }

    pub async fn next_sequence_number(&self, address: &str) -> Result<Option<u64>, SharedError> {
        let value = self
            .call_first_available(
                &self.transaction_methods(
                    "txpool.next_sequence_number2",
                    "txpool.next_sequence_number",
                ),
                json!([address]),
            )
            .await?;
        Ok(value.and_then(|entry| parse_u64(&entry)))
    }

    pub async fn dry_run_raw(
        &self,
        raw_txn_bcs_hex: &str,
        sender_public_key: &str,
    ) -> Result<Value, SharedError> {
        self.call_first_available_value(
            &self.transaction_methods("contract2.dry_run_raw", "contract.dry_run_raw"),
            json!([raw_txn_bcs_hex, sender_public_key]),
        )
        .await
    }

    pub async fn submit_signed_transaction(
        &self,
        signed_txn_bcs_hex: &str,
    ) -> Result<String, SharedError> {
        let hash_value = self
            .call_first_available_value(
                &self.transaction_methods(
                    "txpool.submit_hex_transaction2",
                    "txpool.submit_hex_transaction",
                ),
                json!([signed_txn_bcs_hex]),
            )
            .await?;
        stringify_json(&hash_value).ok_or_else(|| {
            SharedError::new(
                SharedErrorCode::RpcUnavailable,
                "submission RPC returned an invalid transaction hash",
            )
        })
    }

    pub async fn method_exists(&self, method: &str, params: Value) -> Result<bool, SharedError> {
        match self.call_value(method, params).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => Ok(false),
            Err(_) => Ok(true),
        }
    }

    async fn supports_transaction_lookup(&self) -> Result<bool, SharedError> {
        for method in self.transaction_methods("chain.get_transaction2", "chain.get_transaction") {
            if self.method_exists(method, json!(["0x0000000000000000000000000000000000000000000000000000000000000000", { "decode": true }])).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn supports_submission(&self) -> Result<bool, SharedError> {
        let gas_price = self.method_exists("txpool.gas_price", json!([])).await?;
        let sequence = self
            .method_exists(
                "txpool.next_sequence_number2",
                json!(["0x00000000000000000000000000000000"]),
            )
            .await?
            || self
                .method_exists(
                    "txpool.next_sequence_number",
                    json!(["0x00000000000000000000000000000000"]),
                )
                .await?;
        let submit = self
            .method_exists("txpool.submit_hex_transaction2", json!(["0x00"]))
            .await?
            || self
                .method_exists("txpool.submit_hex_transaction", json!(["0x00"]))
                .await?;
        Ok(gas_price && sequence && submit)
    }

    async fn supports_raw_dry_run(&self) -> Result<bool, SharedError> {
        for method in self.transaction_methods("contract2.dry_run_raw", "contract.dry_run_raw") {
            if self.method_exists(method, json!(["0x00", "0x00"])).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn transaction_methods<'a>(&self, preferred: &'a str, fallback: &'a str) -> Vec<&'a str> {
        match self.vm_profile {
            VmProfile::Vm2Only => vec![preferred],
            VmProfile::LegacyCompatible => vec![fallback, preferred],
            VmProfile::Auto => vec![preferred, fallback],
        }
    }

    async fn cached_value<F, Fut>(
        &self,
        cache: &TimedValueCache,
        key: &str,
        loader: F,
    ) -> Result<Value, SharedError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Value, SharedError>>,
    {
        if let Some(value) = cache.get(key).await {
            return Ok(value);
        }
        let value = loader().await?;
        cache.insert(key.to_owned(), value.clone()).await;
        Ok(value)
    }

    async fn call_first_available_value(
        &self,
        methods: &[&str],
        params: Value,
    ) -> Result<Value, SharedError> {
        for method in methods {
            match self.call_value(method, params.clone()).await {
                Ok(value) => return Ok(value),
                Err(error) if error.code == SharedErrorCode::UnsupportedOperation => continue,
                Err(error) => return Err(error),
            }
        }
        Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            format!(
                "none of the candidate rpc methods are available: {}",
                methods.join(", ")
            ),
        ))
    }

    async fn call_first_available(
        &self,
        methods: &[&str],
        params: Value,
    ) -> Result<Option<Value>, SharedError> {
        for method in methods {
            match self.call::<Option<Value>>(method, params.clone()).await {
                Ok(value) => return Ok(value),
                Err(error) if error.code == SharedErrorCode::UnsupportedOperation => continue,
                Err(error) => return Err(error),
            }
        }
        Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            format!(
                "none of the candidate rpc methods are available: {}",
                methods.join(", ")
            ),
        ))
    }

    async fn call_first_available_vec(
        &self,
        methods: &[&str],
        params: Value,
    ) -> Result<Vec<Value>, SharedError> {
        for method in methods {
            match self.call::<Vec<Value>>(method, params.clone()).await {
                Ok(value) => return Ok(value),
                Err(error) if error.code == SharedErrorCode::UnsupportedOperation => continue,
                Err(error) => return Err(error),
            }
        }
        Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            format!(
                "none of the candidate rpc methods are available: {}",
                methods.join(", ")
            ),
        ))
    }

    async fn optional_call_value(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Option<Value>, SharedError> {
        match self.call(method, params).await {
            Ok(value) => Ok(value),
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn call_value(&self, method: &str, params: Value) -> Result<Value, SharedError> {
        self.call(method, params).await
    }

    async fn call<T>(&self, method: &str, params: Value) -> Result<T, SharedError>
    where
        T: DeserializeOwned,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let response = self
            .http
            .post(self.endpoint.clone())
            .json(&RpcRequest {
                jsonrpc: "2.0",
                id,
                method,
                params,
            })
            .send()
            .await
            .map_err(|error| {
                SharedError::retryable(
                    SharedErrorCode::NodeUnavailable,
                    format!("failed to reach rpc endpoint: {error}"),
                )
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|error| {
            SharedError::retryable(
                SharedErrorCode::RpcUnavailable,
                format!("failed to read rpc response body: {error}"),
            )
        })?;
        if !status.is_success() {
            return Err(SharedError::retryable(
                SharedErrorCode::RpcUnavailable,
                format!("rpc endpoint returned HTTP status {status}"),
            )
            .with_details(json!({ "body": body })));
        }

        let envelope: RpcEnvelope<T> = serde_json::from_str(&body).map_err(|error| {
            SharedError::new(
                SharedErrorCode::RpcUnavailable,
                format!("invalid rpc response envelope: {error}"),
            )
            .with_details(json!({ "body": body }))
        })?;

        match (envelope.result, envelope.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => {
                if error.code == -32601 {
                    return Err(SharedError::new(
                        SharedErrorCode::UnsupportedOperation,
                        format!("rpc method {method} is not available"),
                    ));
                }
                let code = map_rpc_error_code(&error.message);
                let retryable = matches!(
                    code,
                    SharedErrorCode::NodeUnavailable
                        | SharedErrorCode::RpcUnavailable
                        | SharedErrorCode::SubmissionUnknown
                );
                let shared = if retryable {
                    SharedError::retryable(code, error.message)
                } else {
                    SharedError::new(code, error.message)
                };
                Err(shared.with_details(json!({
                    "rpc_code": error.code,
                    "rpc_data": error.data,
                    "method": method,
                })))
            }
            _ => Err(SharedError::new(
                SharedErrorCode::RpcUnavailable,
                format!("rpc method {method} returned neither result nor error"),
            )),
        }
    }
}

#[derive(Debug)]
struct TimedValueCache {
    ttl: Duration,
    capacity: usize,
    entries: RwLock<HashMap<String, CachedValue>>,
}

#[derive(Debug, Clone)]
struct CachedValue {
    inserted_at: Instant,
    value: Value,
}

impl TimedValueCache {
    fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity,
            entries: RwLock::new(HashMap::new()),
        }
    }

    async fn get(&self, key: &str) -> Option<Value> {
        let entries = self.entries.read().await;
        entries.get(key).and_then(|entry| {
            if entry.inserted_at.elapsed() <= self.ttl {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    async fn insert(&self, key: String, value: Value) {
        let mut entries = self.entries.write().await;
        if entries.len() >= self.capacity {
            if let Some(first_key) = entries.keys().next().cloned() {
                entries.remove(&first_key);
            }
        }
        entries.insert(
            key,
            CachedValue {
                inserted_at: Instant::now(),
                value,
            },
        );
    }
}

#[derive(Debug, Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope<T> {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<T>,
    error: Option<RpcFailure>,
}

#[derive(Debug, Deserialize)]
struct RpcFailure {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

fn map_rpc_error_code(message: &str) -> SharedErrorCode {
    let lower = message.to_ascii_lowercase();
    if lower.contains("expired") {
        SharedErrorCode::TransactionExpired
    } else if lower.contains("sequence") && (lower.contains("too old") || lower.contains("stale")) {
        SharedErrorCode::SequenceNumberStale
    } else if lower.contains("may have reached the endpoint")
        || lower.contains("timed out after submission")
    {
        SharedErrorCode::SubmissionUnknown
    } else if lower.contains("connection") || lower.contains("unavailable") {
        SharedErrorCode::NodeUnavailable
    } else {
        SharedErrorCode::SubmissionFailed
    }
}

fn parse_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    }
}

fn stringify_json(value: &Value) -> Option<String> {
    match value {
        Value::String(string) => Some(string.clone()),
        Value::Object(_) | Value::Array(_) => Some(value.to_string()),
        _ => None,
    }
}
