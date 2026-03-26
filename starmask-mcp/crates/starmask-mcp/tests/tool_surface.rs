mod support;

use std::{collections::BTreeSet, env, path::PathBuf};

use pretty_assertions::assert_eq;
use serde_json::json;
use starmask_mcp::{
    StarmaskMcpServer, WalletListAccountsRequest, WalletListInstancesRequest, default_socket_path,
};
use starmask_types::{WalletListAccountsResult, WalletListInstancesResult};

use self::support::{
    FakeDaemonClient, FakeDaemonResponses, sample_wallet_account_group,
    sample_wallet_instance_summary, wallet_instance_id,
};

#[test]
fn advertised_tools_expose_expected_wallet_surface() {
    let wallet_instance_id = wallet_instance_id();
    let server = StarmaskMcpServer::new(FakeDaemonClient::with_responses(FakeDaemonResponses {
        wallet_list_instances: Some(WalletListInstancesResult {
            wallet_instances: vec![sample_wallet_instance_summary(&wallet_instance_id)],
        }),
        wallet_list_accounts: Some(WalletListAccountsResult {
            wallet_instances: vec![sample_wallet_account_group(&wallet_instance_id)],
        }),
        ..Default::default()
    }));
    let tool_names = server
        .advertised_tools()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        tool_names,
        BTreeSet::from([
            "wallet_cancel_request".to_owned(),
            "wallet_get_public_key".to_owned(),
            "wallet_get_request_status".to_owned(),
            "wallet_list_accounts".to_owned(),
            "wallet_list_instances".to_owned(),
            "wallet_request_sign_transaction".to_owned(),
            "wallet_sign_message".to_owned(),
            "wallet_status".to_owned(),
        ])
    );
}

#[tokio::test]
async fn call_tool_json_list_accounts_uses_structured_request() {
    let wallet_instance_id = wallet_instance_id();
    let expected = WalletListAccountsResult {
        wallet_instances: vec![sample_wallet_account_group(&wallet_instance_id)],
    };
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        wallet_list_accounts: Some(expected.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json(
            "wallet_list_accounts",
            Some(
                json!({
                    "wallet_instance_id": wallet_instance_id.as_str(),
                    "include_public_key": true,
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_list_accounts,
        Some(WalletListAccountsRequest {
            wallet_instance_id: Some(wallet_instance_id),
            include_public_key: true,
        })
    );
    assert_eq!(
        result,
        serde_json::to_value(expected).expect("result should serialize")
    );
}

#[tokio::test]
async fn call_tool_json_list_instances_requests_all_instances() {
    let wallet_instance_id = wallet_instance_id();
    let expected = WalletListInstancesResult {
        wallet_instances: vec![sample_wallet_instance_summary(&wallet_instance_id)],
    };
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        wallet_list_instances: Some(expected.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json("wallet_list_instances", None)
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_list_instances,
        Some(WalletListInstancesRequest {
            connected_only: false,
        })
    );
    assert_eq!(
        result,
        serde_json::to_value(expected).expect("result should serialize")
    );
}

#[test]
fn default_socket_path_matches_platform_convention() {
    let home = PathBuf::from(env::var_os("HOME").expect("HOME should be set for test runtime"));
    let expected = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("StarcoinMCP")
            .join("run")
            .join("starmaskd.sock")
    } else {
        home.join(".local")
            .join("state")
            .join("starcoin-mcp")
            .join("starmaskd.sock")
    };

    assert_eq!(default_socket_path(), expected);
}
