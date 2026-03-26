use std::collections::BTreeSet;

use httpmock::MockServer;
use serde_json::Value;
use starcoin_node_mcp_server::{StarcoinNodeMcpServer, serve_stdio_with_config};
use starcoin_node_mcp_test_support::{
    mock_abi_methods_not_found, mock_json_rpc_result, mock_method_not_found, mock_probe_metadata,
    mock_transaction_event_methods_not_found, mock_transaction_info_methods_not_found,
    mock_view_methods_not_found, runtime_config, runtime_config_with_endpoint,
};
use starcoin_node_mcp_types::{Mode, VmProfile};

#[tokio::test]
async fn bootstrap_builds_server_from_runtime_config() {
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

    let mcp = StarcoinNodeMcpServer::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("bootstrap should succeed");

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
}

#[tokio::test]
async fn serve_stdio_with_config_surfaces_config_validation_errors() {
    let mut config = runtime_config_with_endpoint(
        "http://127.0.0.1:1",
        Mode::Transaction,
        VmProfile::Auto,
        true,
    );
    config.expected_chain_id = None;

    let error = serve_stdio_with_config(config)
        .await
        .expect_err("invalid config should fail before entering stdio serve loop");

    assert!(
        error
            .to_string()
            .contains("transaction mode requires expected_chain_id")
    );
}
