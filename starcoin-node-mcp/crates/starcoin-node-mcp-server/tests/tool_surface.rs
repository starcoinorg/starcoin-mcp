use std::{collections::BTreeSet, path::PathBuf, time::Duration};

use httpmock::{MockServer, prelude::*};
use serde_json::{Value, json};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_server::StarcoinNodeMcpServer;
use starcoin_node_mcp_types::{Mode, RuntimeConfig, VmProfile};
use url::Url;

#[tokio::test]
async fn advertised_tools_hide_capability_gated_surfaces() {
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
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
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
    mock_json_rpc_error(&server, "chain.get_events", -32601, "method not found");
    mock_json_rpc_error(
        &server,
        "state.get_account_state",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(&server, "state.list_resource", -32601, "method not found");
    mock_json_rpc_error(&server, "state.list_code", -32601, "method not found");
    mock_json_rpc_error(
        &server,
        "contract2.resolve_function",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract.resolve_function",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract2.resolve_module",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract.resolve_module",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract2.resolve_struct",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract.resolve_struct",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(&server, "contract2.call_v2", -32601, "method not found");
    mock_json_rpc_error(&server, "contract.call_v2", -32601, "method not found");

    let app = AppContext::bootstrap(runtime_config(&server))
        .await
        .expect("app should bootstrap");
    let mcp = StarcoinNodeMcpServer::new(app);
    let tool_names = mcp
        .advertised_tools()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<BTreeSet<_>>();

    assert!(tool_names.contains("chain_status"));
    assert!(tool_names.contains("node_health"));
    assert!(tool_names.contains("get_block"));
    assert!(tool_names.contains("get_transaction"));
    assert!(!tool_names.contains("list_blocks"));
    assert!(!tool_names.contains("watch_transaction"));
    assert!(!tool_names.contains("get_account_overview"));
}

#[tokio::test]
async fn call_tool_json_serializes_pending_transaction_without_events() {
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
    mock_json_rpc_result(
        &server,
        "chain.get_transaction2",
        json!({ "status": "Pending" }),
    );
    mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
    mock_json_rpc_error(&server, "chain.get_events", -32601, "method not found");
    mock_json_rpc_error(
        &server,
        "state.get_account_state",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(&server, "state.list_resource", -32601, "method not found");
    mock_json_rpc_error(&server, "state.list_code", -32601, "method not found");
    mock_json_rpc_error(
        &server,
        "contract2.resolve_function",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract.resolve_function",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract2.resolve_module",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract.resolve_module",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract2.resolve_struct",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "contract.resolve_struct",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(&server, "contract2.call_v2", -32601, "method not found");
    mock_json_rpc_error(&server, "contract.call_v2", -32601, "method not found");

    let app = AppContext::bootstrap(runtime_config(&server))
        .await
        .expect("app should bootstrap");
    let mcp = StarcoinNodeMcpServer::new(app);
    let result = mcp
        .call_tool_json(
            "get_transaction",
            Some(
                json!({
                    "txn_hash": "0x1",
                    "include_events": true,
                    "decode": true,
                })
                .as_object()
                .expect("tool args should be object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(result["status_summary"]["found"], true);
    assert_eq!(result["status_summary"]["confirmed"], false);
    assert_eq!(result["events"], json!([]));
}

fn runtime_config(server: &MockServer) -> RuntimeConfig {
    RuntimeConfig {
        rpc_endpoint_url: Url::parse(&server.url("/")).expect("mock url should parse"),
        mode: Mode::ReadOnly,
        vm_profile: VmProfile::Auto,
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

fn mock_json_rpc_result(server: &MockServer, method: &str, result: Value) {
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
    });
}

fn mock_json_rpc_error(server: &MockServer, method: &str, code: i64, message: &str) {
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
    });
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
