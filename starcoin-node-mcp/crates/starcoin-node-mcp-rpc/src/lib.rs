#![forbid(unsafe_code)]

mod cache;
mod probe;
#[cfg(test)]
mod tests;
mod tls;

use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue},
};
use rustls::{
    ClientConfig, DEFAULT_VERSIONS, DigitallySignedStruct, Error as TlsError, RootCertStore,
    SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use starcoin_node_mcp_types::{
    EffectiveProbe, RuntimeConfig, SharedError, SharedErrorCode, VmProfile,
};
use tokio::sync::RwLock;
use url::Url;
use x509_parser::{certificate::X509Certificate, prelude::FromDer};

use tls::build_http_client;

pub(crate) use cache::TimedValueCache;

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
        config.validate()?;
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
        let (http, endpoint) = build_http_client(config, default_headers)?;

        Ok(Self {
            endpoint,
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
            self.chain_info_uncached().await
        })
        .await
    }

    pub async fn chain_info_uncached(&self) -> Result<Value, SharedError> {
        self.call_value("chain.info", json!([])).await
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
        if self.vm_profile == VmProfile::Vm2Only {
            return self.account_state_from_vm2_resources(address).await;
        }
        match self.call("state.get_account_state", json!([address])).await {
            Ok(Some(state)) => Ok(Some(state)),
            Ok(None) => Ok(self.account_state_from_vm2_resources(address).await.unwrap_or(None)),
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => {
                Ok(self.account_state_from_vm2_resources(address).await.unwrap_or(None))
            }
            Err(error) => Err(error),
        }
    }

    pub async fn get_state_root(&self) -> Result<Value, SharedError> {
        if self.vm_profile == VmProfile::Vm2Only {
            return self.call_value("state2.get_state_root", json!([])).await;
        }
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
        if self.vm_profile == VmProfile::Vm2Only {
            return self
                .list_resources_via_vm2(
                    address,
                    decode,
                    start_index,
                    max_size,
                    state_root,
                    resource_types,
                )
                .await;
        }
        let legacy_result = self
            .call_value(
                "state.list_resource",
                json!([
                    address,
                    {
                        "decode": decode,
                        "state_root": state_root.clone(),
                        "start_index": start_index,
                        "max_size": max_size,
                        "resource_types": if resource_types.is_empty() { Value::Null } else { json!(resource_types) }
                    }
                ]),
            )
            .await;

        match legacy_result {
            Ok(value) if resource_entries_are_non_empty(&value) => Ok(value),
            Ok(value) => match self
                .list_resources_via_vm2(
                    address,
                    decode,
                    start_index,
                    max_size,
                    state_root.clone(),
                    resource_types,
                )
                .await
            {
                Ok(vm2_value) => Ok(vm2_value),
                Err(_) => Ok(value),
            },
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => {
                self.list_resources_via_vm2(
                    address,
                    decode,
                    start_index,
                    max_size,
                    state_root,
                    resource_types,
                )
                .await
            }
            Err(error) => Err(error),
        }
    }

    pub async fn list_code(
        &self,
        address: &str,
        resolve: bool,
        state_root: Option<String>,
    ) -> Result<Value, SharedError> {
        if self.vm_profile == VmProfile::Vm2Only {
            return self
                .call_value(
                    "state2.list_code",
                    json!([
                        address,
                        {
                            "resolve": resolve,
                            "state_root": state_root,
                        }
                    ]),
                )
                .await;
        }
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
        extract_submission_hash(&hash_value)
    }

    pub async fn method_exists(&self, method: &str, params: Value) -> Result<bool, SharedError> {
        match self.call_value(method, params).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => Ok(false),
            Err(error) => Err(error),
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

    async fn list_resources_via_vm2(
        &self,
        address: &str,
        decode: bool,
        start_index: u64,
        max_size: u64,
        state_root: Option<String>,
        resource_types: &[String],
    ) -> Result<Value, SharedError> {
        self.call_value(
            "state2.list_resource",
            json!([
                address,
                {
                    "decode": decode,
                    "state_root": state_root,
                    "start_index": start_index,
                    "max_size": max_size,
                    "resource_types": if resource_types.is_empty() { Value::Null } else { json!(resource_types) },
                    "primary_fungible_store": {}
                }
            ]),
        )
        .await
    }

    async fn account_state_from_vm2_resources(
        &self,
        address: &str,
    ) -> Result<Option<Value>, SharedError> {
        let resources = self
            .list_resources_via_vm2(address, true, 0, 32, None, &[])
            .await?;
        Ok(synthesize_account_state_from_resources(&resources))
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

        let envelope: RpcEnvelope = serde_json::from_str(&body).map_err(|error| {
            SharedError::new(
                SharedErrorCode::RpcUnavailable,
                format!("invalid rpc response envelope: {error}"),
            )
            .with_details(json!({ "body": body }))
        })?;

        match envelope.error {
            None => serde_json::from_value(envelope.result).map_err(|error| {
                SharedError::new(
                    SharedErrorCode::RpcUnavailable,
                    format!("invalid rpc result payload: {error}"),
                )
                .with_details(json!({ "body": body, "method": method }))
            }),
            Some(error) => {
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
        }
    }
}

fn extract_submission_hash(hash_value: &Value) -> Result<String, SharedError> {
    hash_value.as_str().map(str::to_owned).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            "submission RPC returned an invalid transaction hash",
        )
    })
}

#[derive(Debug, Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    #[serde(default)]
    result: Value,
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

fn resource_entries_are_non_empty(value: &Value) -> bool {
    value.get("resources")
        .and_then(Value::as_object)
        .map(|entries| !entries.is_empty())
        .unwrap_or(false)
}

fn synthesize_account_state_from_resources(resources: &Value) -> Option<Value> {
    let entries = resources.get("resources")?.as_object()?;
    if entries.is_empty() {
        return None;
    }

    let sequence_number = entries
        .iter()
        .find(|(name, _)| name.contains("::account::Account"))
        .and_then(|(_, resource)| resource.get("json"))
        .and_then(|resource| resource.get("sequence_number"))
        .and_then(parse_u64);

    let mut summary = serde_json::Map::new();
    if let Some(sequence_number) = sequence_number {
        summary.insert("sequence_number".to_owned(), json!(sequence_number));
    }

    Some(Value::Object(summary))
}
