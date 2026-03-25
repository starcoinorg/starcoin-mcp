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

fn sample_runtime_config(server: &MockServer, mode: Mode, vm_profile: VmProfile) -> RuntimeConfig {
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
