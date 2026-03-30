use std::{path::PathBuf, time::Duration};

use httpmock::{Mock, MockServer, prelude::*};
use serde_json::{Value, json};
use starcoin_node_mcp_types::{Mode, RuntimeConfig, VmProfile};
use url::Url;

pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

pub fn runtime_config(
    server: &MockServer,
    mode: Mode,
    vm_profile: VmProfile,
    allow_submit_without_prior_simulation: bool,
) -> RuntimeConfig {
    runtime_config_with_endpoint(
        &server.url("/"),
        mode,
        vm_profile,
        allow_submit_without_prior_simulation,
    )
}

pub fn runtime_config_with_endpoint(
    endpoint: &str,
    mode: Mode,
    vm_profile: VmProfile,
    allow_submit_without_prior_simulation: bool,
) -> RuntimeConfig {
    RuntimeConfig {
        rpc_endpoint_url: Url::parse(endpoint).expect("valid url"),
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
        allow_submit_without_prior_simulation,
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

pub fn sample_node_info() -> Value {
    json!({
        "net": { "Builtin": "Main" },
        "now_seconds": 120,
    })
}

pub fn sample_chain_info() -> Value {
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

pub fn mock_probe_metadata(server: &MockServer) {
    mock_json_rpc_result(server, "node.status", json!(true));
    mock_json_rpc_result(server, "chain.info", sample_chain_info());
    mock_json_rpc_result(server, "node.info", sample_node_info());
}

pub fn mock_block_lookup_probe<'a>(server: &'a MockServer, result: Value) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"chain.get_block_by_number\"")
            .body_contains("[0,");
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

pub fn mock_transaction_lookup_probe<'a>(
    server: &'a MockServer,
    method: &str,
    result: Value,
) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains(&format!("\"method\":\"{method}\""))
            .body_contains(
                "\"0x0000000000000000000000000000000000000000000000000000000000000000\"",
            );
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

pub fn mock_json_rpc_result<'a>(server: &'a MockServer, method: &str, result: Value) -> Mock<'a> {
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

pub fn mock_json_rpc_result_with_params<'a>(
    server: &'a MockServer,
    method: &str,
    params: Value,
    result: Value,
) -> Mock<'a> {
    let params = serde_json::to_string(&params).expect("params should serialize");
    server.mock(move |when, then| {
        when.method(POST)
            .path("/")
            .body_contains(&format!("\"method\":\"{method}\""))
            .body_contains(&format!("\"params\":{params}"));
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

pub fn mock_json_rpc_error<'a>(
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

pub fn mock_method_not_found<'a>(server: &'a MockServer, method: &str) -> Mock<'a> {
    mock_json_rpc_error(server, method, METHOD_NOT_FOUND, "method not found")
}

pub fn mock_http_error<'a>(
    server: &'a MockServer,
    method: &str,
    status: u16,
    body: &str,
) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains(&format!("\"method\":\"{method}\""));
        then.status(status).body(body);
    })
}

pub fn mock_transaction_info_methods_not_found(server: &MockServer) {
    mock_method_not_found(server, "chain.get_transaction_info2");
    mock_method_not_found(server, "chain.get_transaction_info");
}

pub fn mock_transaction_event_methods_not_found(server: &MockServer) {
    mock_method_not_found(server, "chain.get_events_by_txn_hash2");
    mock_method_not_found(server, "chain.get_events_by_txn_hash");
}

pub fn mock_abi_methods_not_found(server: &MockServer) {
    mock_method_not_found(server, "contract2.resolve_function");
    mock_method_not_found(server, "contract.resolve_function");
    mock_method_not_found(server, "contract2.resolve_module");
    mock_method_not_found(server, "contract.resolve_module");
    mock_method_not_found(server, "contract2.resolve_struct");
    mock_method_not_found(server, "contract.resolve_struct");
}

pub fn mock_view_methods_not_found(server: &MockServer) {
    mock_method_not_found(server, "contract2.call_v2");
    mock_method_not_found(server, "contract.call_v2");
}

pub fn mock_submit_probe_invalid_params<'a>(server: &'a MockServer, method: &str) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains(&format!("\"method\":\"{method}\""))
            .body_contains("\"params\":[]");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "error": {
                        "code": INVALID_PARAMS,
                        "message": "invalid params",
                    }
                })
                .to_string(),
            );
    })
}

pub fn mock_txpool_sequence_probe<'a>(
    server: &'a MockServer,
    method: &str,
    result: Value,
) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains(&format!("\"method\":\"{method}\""))
            .body_contains("\"0x00000000000000000000000000000000\"");
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
