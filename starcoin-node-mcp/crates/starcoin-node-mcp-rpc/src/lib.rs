#![forbid(unsafe_code)]

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
        extract_submission_hash(&hash_value)
    }

    pub async fn method_exists(&self, method: &str, params: Value) -> Result<bool, SharedError> {
        match self.call_value(method, params).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == SharedErrorCode::UnsupportedOperation => Ok(false),
            Err(error) => Err(error),
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
            Err(_) => Ok(true),
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

fn build_http_client(
    config: &RuntimeConfig,
    default_headers: HeaderMap,
) -> anyhow::Result<(Client, Url)> {
    let mut endpoint = config.rpc_endpoint_url.clone();
    let mut builder = Client::builder()
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
        .default_headers(default_headers);

    let tls_server_name = config
        .tls_server_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if (tls_server_name.is_some() || !config.tls_pinned_spki_sha256.is_empty())
        && endpoint.scheme() != "https"
    {
        return Err(anyhow!(
            "tls_server_name and tls_pinned_spki_sha256 require an https rpc endpoint"
        ));
    }

    if endpoint.scheme() == "https" {
        if let Some(server_name) = tls_server_name {
            let original_host = endpoint
                .host_str()
                .context("rpc endpoint is missing host")?;
            if !original_host.eq_ignore_ascii_case(server_name) {
                let addrs = resolve_endpoint_addrs(&endpoint)?;
                endpoint
                    .set_host(Some(server_name))
                    .map_err(|_| anyhow!("invalid tls_server_name {server_name}"))?;
                builder = builder.resolve_to_addrs(server_name, &addrs);
            }
        }

        if tls_server_name.is_some()
            || !config.tls_pinned_spki_sha256.is_empty()
            || config.allow_insecure_remote_transport
        {
            builder = builder.use_preconfigured_tls(build_rustls_client_config(config)?);
        }
    }

    let http = builder.build().context("failed to build rpc http client")?;
    Ok((http, endpoint))
}

fn build_rustls_client_config(config: &RuntimeConfig) -> anyhow::Result<ClientConfig> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let supported_algorithms = provider.signature_verification_algorithms;
    let pins = parse_spki_pins(&config.tls_pinned_spki_sha256)?;
    let base_verifier = if config.allow_insecure_remote_transport {
        None
    } else {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let verifier: Arc<dyn ServerCertVerifier> =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .context("failed to build TLS verifier")?;
        Some(verifier)
    };
    let verifier = Arc::new(ConfiguredServerCertVerifier {
        base_verifier,
        pins,
        supported_algorithms,
    });
    let tls = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(DEFAULT_VERSIONS)
        .context("invalid TLS protocol version configuration")?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(tls)
}

fn parse_spki_pins(pins: &[String]) -> anyhow::Result<Vec<[u8; 32]>> {
    pins.iter().map(|pin| parse_spki_pin(pin)).collect()
}

fn parse_spki_pin(pin: &str) -> anyhow::Result<[u8; 32]> {
    let normalized = pin
        .trim()
        .strip_prefix("sha256/")
        .unwrap_or(pin.trim())
        .trim_start_matches("0x");

    if let Ok(bytes) = hex::decode(normalized) {
        if bytes.len() == 32 {
            return Ok(bytes
                .try_into()
                .expect("32-byte hex-encoded SPKI hash should fit"));
        }
    }

    let bytes = BASE64_STANDARD
        .decode(normalized)
        .with_context(|| format!("invalid tls_pinned_spki_sha256 value: {pin}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("tls_pinned_spki_sha256 entries must decode to 32 bytes"))
}

fn resolve_endpoint_addrs(endpoint: &Url) -> anyhow::Result<Vec<SocketAddr>> {
    let host = endpoint
        .host_str()
        .context("rpc endpoint is missing host")?;
    let port = endpoint
        .port_or_known_default()
        .context("rpc endpoint is missing a usable port")?;
    let addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve rpc endpoint host {host}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(anyhow!(
            "resolved no socket addresses for rpc endpoint host {host}"
        ));
    }
    Ok(addrs)
}

fn extract_submission_hash(hash_value: &Value) -> Result<String, SharedError> {
    hash_value.as_str().map(str::to_owned).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            "submission RPC returned an invalid transaction hash",
        )
    })
}

#[derive(Debug)]
struct ConfiguredServerCertVerifier {
    base_verifier: Option<Arc<dyn ServerCertVerifier>>,
    pins: Vec<[u8; 32]>,
    supported_algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for ConfiguredServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if let Some(base_verifier) = &self.base_verifier {
            base_verifier.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                ocsp_response,
                now,
            )?;
        }

        if !self.pins.is_empty() {
            let actual_pin = extract_spki_pin_from_certificate(end_entity)?;
            if !self.pins.iter().any(|expected| *expected == actual_pin) {
                return Err(TlsError::General(
                    "server certificate SPKI pin mismatch".to_owned(),
                ));
            }
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algorithms.supported_schemes()
    }
}

fn extract_spki_pin_from_certificate(cert: &CertificateDer<'_>) -> Result<[u8; 32], TlsError> {
    let (_, certificate) = X509Certificate::from_der(cert.as_ref()).map_err(|_| {
        TlsError::General("failed to parse server certificate for SPKI pinning".to_owned())
    })?;
    let digest = Sha256::digest(certificate.public_key().raw);
    let mut fingerprint = [0u8; 32];
    fingerprint.copy_from_slice(&digest);
    Ok(fingerprint)
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
        if self.capacity == 0 {
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::NodeRpcClient;
    use httpmock::{Mock, prelude::*};
    use serde_json::{Value, json};
    use starcoin_node_mcp_types::{Mode, RuntimeConfig, SharedErrorCode, VmProfile};
    use std::{path::PathBuf, time::Duration};
    use url::Url;

    #[tokio::test]
    async fn probe_classifies_optional_capabilities_and_legacy_fallbacks() {
        let server = MockServer::start();
        mock_json_rpc_result(&server, "node.status", json!(true));
        mock_json_rpc_result(&server, "chain.info", sample_chain_info());
        mock_json_rpc_result(&server, "node.info", sample_node_info());
        mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
        mock_json_rpc_error(
            &server,
            "chain.get_blocks_by_number",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(
            &server,
            "chain.get_transaction2",
            -32601,
            "method not found",
        );
        mock_json_rpc_result(&server, "chain.get_transaction", Value::Null);
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
        mock_json_rpc_error(
            &server,
            "state.get_account_state",
            -32601,
            "method not found",
        );
        mock_json_rpc_error(&server, "chain.get_events", -32601, "method not found");
        mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
        mock_json_rpc_error(&server, "state.list_code", -32601, "method not found");
        mock_json_rpc_error(
            &server,
            "contract2.resolve_function",
            -32601,
            "method not found",
        );
        mock_json_rpc_result(
            &server,
            "contract.resolve_function",
            json!({ "name": "balance" }),
        );
        mock_json_rpc_error(
            &server,
            "contract2.resolve_module",
            -32601,
            "method not found",
        );
        mock_json_rpc_result(
            &server,
            "contract.resolve_module",
            json!({ "name": "Account" }),
        );
        mock_json_rpc_error(
            &server,
            "contract2.resolve_struct",
            -32601,
            "method not found",
        );
        mock_json_rpc_result(
            &server,
            "contract.resolve_struct",
            json!({ "name": "Account" }),
        );
        mock_json_rpc_result(&server, "contract.call_v2", json!([]));

        let client = NodeRpcClient::new(&sample_runtime_config(
            &server,
            Mode::ReadOnly,
            VmProfile::LegacyCompatible,
        ))
        .expect("rpc client should build");
        let probe = client.probe(false).await.expect("probe should succeed");

        assert!(probe.supports_block_lookup);
        assert!(!probe.supports_block_listing);
        assert!(probe.supports_transaction_lookup);
        assert!(!probe.supports_transaction_info_lookup);
        assert!(!probe.supports_transaction_events_by_hash);
        assert!(!probe.supports_account_state_lookup);
        assert!(!probe.supports_events_query);
        assert!(probe.supports_resource_listing);
        assert!(!probe.supports_module_listing);
        assert!(probe.supports_abi_resolution);
        assert!(probe.supports_view_call);
        assert!(!probe.supports_transaction_submission);
        assert!(!probe.supports_raw_dry_run);
    }

    #[tokio::test]
    async fn chain_info_cache_reuses_value_but_uncached_bypasses_it() {
        let server = MockServer::start();
        let chain_info = mock_json_rpc_result(&server, "chain.info", sample_chain_info());
        let client = NodeRpcClient::new(&sample_runtime_config(
            &server,
            Mode::ReadOnly,
            VmProfile::Auto,
        ))
        .expect("rpc client should build");

        client
            .chain_info()
            .await
            .expect("first cached read should succeed");
        client
            .chain_info()
            .await
            .expect("second cached read should reuse cache");
        assert_eq!(chain_info.hits(), 1);

        client
            .chain_info_uncached()
            .await
            .expect("uncached read should bypass cache");
        assert_eq!(chain_info.hits(), 2);
    }

    #[tokio::test]
    async fn transaction_probe_detects_submission_and_dry_run_capabilities() {
        let server = MockServer::start();
        mock_json_rpc_result(&server, "node.status", json!(true));
        mock_json_rpc_result(&server, "chain.info", sample_chain_info());
        mock_json_rpc_result(&server, "node.info", sample_node_info());
        mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
        mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
        mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
        mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
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
        mock_json_rpc_error(
            &server,
            "state.get_account_state",
            -32601,
            "method not found",
        );
        mock_json_rpc_result(&server, "chain.get_events", json!([]));
        mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
        mock_json_rpc_result(&server, "state.list_code", json!({ "codes": {} }));
        mock_json_rpc_result(
            &server,
            "contract2.resolve_function",
            json!({ "name": "balance" }),
        );
        mock_json_rpc_result(
            &server,
            "contract2.resolve_module",
            json!({ "name": "Account" }),
        );
        mock_json_rpc_result(
            &server,
            "contract2.resolve_struct",
            json!({ "name": "Account" }),
        );
        mock_json_rpc_result(&server, "contract2.call_v2", json!([]));
        mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
        mock_json_rpc_result(&server, "txpool.next_sequence_number2", json!("0"));
        let submit_probe = server.mock(|when, then| {
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

        let client = NodeRpcClient::new(&sample_runtime_config(
            &server,
            Mode::Transaction,
            VmProfile::Vm2Only,
        ))
        .expect("rpc client should build");
        let probe = client
            .probe(true)
            .await
            .expect("transaction probe should succeed");

        assert!(probe.supports_block_listing);
        assert!(probe.supports_transaction_info_lookup);
        assert!(!probe.supports_transaction_events_by_hash);
        assert!(!probe.supports_account_state_lookup);
        assert!(probe.supports_transaction_submission);
        assert!(probe.supports_raw_dry_run);
        assert_eq!(submit_probe.hits(), 1);
    }

    #[tokio::test]
    async fn method_exists_propagates_transport_errors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .body_contains("\"method\":\"chain.info\"");
            then.status(500).body("boom");
        });

        let client = NodeRpcClient::new(&sample_runtime_config(
            &server,
            Mode::ReadOnly,
            VmProfile::Auto,
        ))
        .expect("rpc client should build");
        let error = client
            .method_exists("chain.info", json!([]))
            .await
            .expect_err("HTTP transport failures should propagate");

        assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn submit_rejects_non_string_hash_payloads() {
        let server = MockServer::start();
        mock_json_rpc_result(
            &server,
            "txpool.submit_hex_transaction2",
            json!({ "hash": "0xabc" }),
        );

        let client = NodeRpcClient::new(&sample_runtime_config(
            &server,
            Mode::Transaction,
            VmProfile::Vm2Only,
        ))
        .expect("rpc client should build");
        let error = client
            .submit_signed_transaction("0x01")
            .await
            .expect_err("non-string submit results should be rejected");

        assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
    }

    #[tokio::test]
    async fn abi_cache_respects_zero_capacity() {
        let server = MockServer::start();
        let abi = mock_json_rpc_result(
            &server,
            "contract2.resolve_function",
            json!({ "name": "balance" }),
        );
        let mut config = sample_runtime_config(&server, Mode::ReadOnly, VmProfile::Vm2Only);
        config.module_cache_max_entries = 0;

        let client = NodeRpcClient::new(&config).expect("rpc client should build");
        client
            .resolve_function_abi("0x1::Account::balance")
            .await
            .expect("first ABI lookup should succeed");
        client
            .resolve_function_abi("0x1::Account::balance")
            .await
            .expect("second ABI lookup should also succeed");

        assert_eq!(abi.hits(), 2);
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

    fn sample_node_info() -> Value {
        json!({
            "net": { "Builtin": "Main" },
            "now_seconds": 120,
        })
    }

    fn sample_chain_info() -> Value {
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

    fn sample_runtime_config(
        server: &MockServer,
        mode: Mode,
        vm_profile: VmProfile,
    ) -> RuntimeConfig {
        RuntimeConfig {
            rpc_endpoint_url: Url::parse(&server.url("/")).expect("mock url should parse"),
            mode,
            vm_profile,
            expected_chain_id: Some(254),
            expected_network: Some("main".to_owned()),
            expected_genesis_hash: Some("0x1".to_owned()),
            require_genesis_hash_match: true,
            connect_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(3),
            startup_probe_timeout: Duration::from_secs(3),
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
            chain_status_cache_ttl: Duration::from_secs(60),
            abi_cache_ttl: Duration::from_secs(300),
            module_cache_max_entries: 128,
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
}
