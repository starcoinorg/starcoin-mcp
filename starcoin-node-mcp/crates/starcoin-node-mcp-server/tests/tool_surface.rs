use std::collections::BTreeSet;

use httpmock::MockServer;
use serde_json::{Value, json};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_server::StarcoinNodeMcpServer;
use starcoin_node_mcp_test_support::{
    mock_abi_methods_not_found, mock_json_rpc_result, mock_method_not_found, mock_probe_metadata,
    mock_transaction_event_methods_not_found, mock_transaction_info_methods_not_found,
    mock_view_methods_not_found, runtime_config,
};
use starcoin_node_mcp_types::{Mode, VmProfile};

#[tokio::test]
async fn advertised_tools_hide_capability_gated_surfaces() {
    let server = MockServer::start();
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_method_not_found(&server, "chain.get_blocks_by_number");
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_transaction_info_methods_not_found(&server);
    mock_transaction_event_methods_not_found(&server);
    mock_method_not_found(&server, "chain.get_events");
    mock_method_not_found(&server, "state.get_account_state");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_abi_methods_not_found(&server);
    mock_view_methods_not_found(&server);

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
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
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_method_not_found(&server, "chain.get_blocks_by_number");
    mock_json_rpc_result(
        &server,
        "chain.get_transaction2",
        json!({ "status": "Pending" }),
    );
    mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
    mock_method_not_found(&server, "chain.get_events");
    mock_method_not_found(&server, "state.get_account_state");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_abi_methods_not_found(&server);
    mock_view_methods_not_found(&server);

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
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

#[tokio::test]
async fn call_view_function_omits_raw_return_values_when_endpoint_only_returns_decoded_values() {
    let server = MockServer::start();
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_method_not_found(&server, "chain.get_blocks_by_number");
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_transaction_info_methods_not_found(&server);
    mock_transaction_event_methods_not_found(&server);
    mock_method_not_found(&server, "chain.get_events");
    mock_method_not_found(&server, "state.get_account_state");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_abi_methods_not_found(&server);
    mock_json_rpc_result(
        &server,
        "contract2.call_v2",
        json!([{
            "type": "u64",
            "value": "7",
        }]),
    );

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("app should bootstrap");
    let mcp = StarcoinNodeMcpServer::new(app);
    let result = mcp
        .call_tool_json(
            "call_view_function",
            Some(
                json!({
                    "function_id": "0x1::Account::balance",
                    "type_args": [],
                    "args": [],
                })
                .as_object()
                .expect("tool args should be object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert!(result.get("return_values").is_none());
    assert_eq!(
        result["decoded_return_values"],
        json!([{
            "type": "u64",
            "value": "7",
        }])
    );
}
