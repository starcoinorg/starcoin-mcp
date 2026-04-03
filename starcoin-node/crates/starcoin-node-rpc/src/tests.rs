use super::NodeRpcClient;
use httpmock::prelude::*;
use serde_json::{Value, json};
use starcoin_node_test_support::{
    mock_json_rpc_result, mock_json_rpc_result_with_params, mock_method_not_found,
    mock_probe_metadata, mock_submit_probe_invalid_params,
    mock_transaction_event_methods_not_found, mock_transaction_info_methods_not_found,
    mock_txpool_sequence_probe, runtime_config, sample_chain_info,
};
use starcoin_node_types::{Mode, SharedErrorCode, VmProfile};

#[tokio::test]
async fn probe_classifies_optional_capabilities_for_vm1_only_surface() {
    let server = MockServer::start();
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_method_not_found(&server, "chain.get_blocks_by_number");
    mock_method_not_found(&server, "chain.get_transaction2");
    mock_json_rpc_result(&server, "chain.get_transaction", Value::Null);
    mock_transaction_info_methods_not_found(&server);
    mock_transaction_event_methods_not_found(&server);
    mock_method_not_found(&server, "state.get_account_state");
    mock_method_not_found(&server, "chain.get_events");
    mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
    mock_method_not_found(&server, "state.list_code");
    mock_method_not_found(&server, "contract2.resolve_function");
    mock_json_rpc_result(
        &server,
        "contract.resolve_function",
        json!({ "name": "balance" }),
    );
    mock_method_not_found(&server, "contract2.resolve_module");
    mock_json_rpc_result(
        &server,
        "contract.resolve_module",
        json!({ "name": "Account" }),
    );
    mock_method_not_found(&server, "contract2.resolve_struct");
    mock_json_rpc_result(
        &server,
        "contract.resolve_struct",
        json!({ "name": "Account" }),
    );
    mock_json_rpc_result(&server, "contract.call_v2", json!([]));

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Vm1Only,
        true,
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
async fn list_resources_vm1_only_keeps_legacy_result_without_vm2_retry() {
    let server = MockServer::start();
    let legacy = mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({ "resources": {} }),
    );
    let vm2 = mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 7
                    }
                }
            }
        }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Vm1Only,
        true,
    ))
    .expect("rpc client should build");
    let resources = client
        .list_resources("0x1", true, 0, 20, None, &[])
        .await
        .expect("vm1_only should return the legacy result as-is");

    assert_eq!(resources, json!({ "resources": {} }));
    assert_eq!(legacy.hits(), 1);
    assert_eq!(vm2.hits(), 0);
}

#[tokio::test]
async fn submit_vm1_only_does_not_retry_on_vm2_surface() {
    let server = MockServer::start();
    mock_method_not_found(&server, "txpool.submit_hex_transaction");
    let vm2 = mock_json_rpc_result(&server, "txpool.submit_hex_transaction2", json!("0xabc"));

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::Transaction,
        VmProfile::Vm1Only,
        true,
    ))
    .expect("rpc client should build");
    let error = client
        .submit_signed_transaction("0x01")
        .await
        .expect_err("vm1_only should fail closed instead of retrying on vm2");

    assert_eq!(error.code, SharedErrorCode::UnsupportedOperation);
    assert_eq!(vm2.hits(), 0);
}

#[tokio::test]
async fn primary_stc_balance_helper_is_disabled_in_vm1_only() {
    let server = MockServer::start();
    let vm2 = mock_json_rpc_result(
        &server,
        "state2.get_resource",
        json!({
            "raw": "0x01",
            "json": {
                "balance": 42
            }
        }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Vm1Only,
        true,
    ))
    .expect("rpc client should build");
    let resource = client
        .get_primary_stc_balance_resource("0x1")
        .await
        .expect("vm1_only should skip vm2-only primary-store helper");

    assert_eq!(resource, None);
    assert_eq!(vm2.hits(), 0);
}

#[tokio::test]
async fn chain_info_cache_reuses_value_but_uncached_bypasses_it() {
    let server = MockServer::start();
    let chain_info = mock_json_rpc_result(&server, "chain.info", sample_chain_info());
    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
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
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
    mock_transaction_event_methods_not_found(&server);
    mock_json_rpc_result(&server, "chain.get_events", json!([]));
    mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params(
            "0x00000000000000000000000000000000",
            true,
            0,
            1,
            None,
            &[],
        ),
        json!({ "resources": {} }),
    );
    mock_json_rpc_result_with_params(
        &server,
        "state2.list_code",
        super::vm2_list_code_params("0x00000000000000000000000000000000", true, None),
        json!({ "codes": {} }),
    );
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
    mock_txpool_sequence_probe(&server, "txpool.next_sequence_number2", json!("0"));
    let submit_probe = mock_submit_probe_invalid_params(&server, "txpool.submit_hex_transaction2");
    mock_json_rpc_result(
        &server,
        "contract2.dry_run_raw",
        json!({ "status": "Executed" }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::Transaction,
        VmProfile::Vm2Only,
        true,
    ))
    .expect("rpc client should build");
    let probe = client
        .probe(true)
        .await
        .expect("transaction probe should succeed");

    assert!(probe.supports_block_listing);
    assert!(probe.supports_transaction_info_lookup);
    assert!(!probe.supports_transaction_events_by_hash);
    assert!(probe.supports_account_state_lookup);
    assert!(probe.supports_resource_listing);
    assert!(probe.supports_module_listing);
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

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
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

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::Transaction,
        VmProfile::Vm2Only,
        true,
    ))
    .expect("rpc client should build");
    let error = client
        .submit_signed_transaction("0x01")
        .await
        .expect_err("non-string submit results should be rejected");

    assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
}

#[tokio::test]
async fn get_account_state_falls_back_to_vm2_resources_when_legacy_state_is_empty() {
    let server = MockServer::start();
    let legacy = mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    let listed = mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params(
            "0x1",
            true,
            0,
            20,
            None,
            &[String::from("0x1::account::Account")],
        ),
        json!({ "resources": {} }),
    );
    let vm2 = mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params(
            "0x1",
            true,
            0,
            20,
            None,
            &[String::from("0x1::account::Account")],
        ),
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 7
                    }
                }
            }
        }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let state = client
        .get_account_state("0x1")
        .await
        .expect("vm2 fallback should succeed")
        .expect("account state should be synthesized");

    assert_eq!(state.get("sequence_number"), Some(&json!(7)));
    assert_eq!(legacy.hits(), 1);
    assert_eq!(listed.hits(), 1);
    assert_eq!(vm2.hits(), 1);
}

#[tokio::test]
async fn list_resources_falls_back_to_vm2_when_legacy_listing_is_empty() {
    let server = MockServer::start();
    let legacy = mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({ "resources": {} }),
    );
    let vm2 = mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 7
                    }
                }
            }
        }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let resources = client
        .list_resources("0x1", true, 0, 20, None, &[])
        .await
        .expect("vm2 listing fallback should succeed");

    assert_eq!(
        resources
            .get("resources")
            .and_then(Value::as_object)
            .map(|entries| entries.len()),
        Some(1)
    );
    assert_eq!(legacy.hits(), 1);
    assert_eq!(vm2.hits(), 1);
}

#[tokio::test]
async fn get_account_state_falls_back_to_vm2_resources_when_legacy_state_lacks_sequence_number() {
    let server = MockServer::start();
    let legacy = mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({
            "sequence_number": Value::Null,
            "storage_roots": [Value::Null, "0xabc"]
        }),
    );
    let listed = mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params(
            "0x1",
            true,
            0,
            20,
            None,
            &[String::from("0x1::account::Account")],
        ),
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 9
                    }
                }
            }
        }),
    );
    let vm2 = mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params("0x1", true, 0, 32, None, &[]),
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 9
                    }
                }
            }
        }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let state = client
        .get_account_state("0x1")
        .await
        .expect("vm2 fallback should succeed")
        .expect("account state should be synthesized");

    assert_eq!(state.get("sequence_number"), Some(&json!(9)));
    assert_eq!(
        state.get("storage_roots"),
        Some(&json!([Value::Null, "0xabc"]))
    );
    assert_eq!(legacy.hits(), 1);
    assert_eq!(listed.hits(), 1);
    assert_eq!(vm2.hits(), 0);
}

#[tokio::test]
async fn get_account_state_uses_legacy_array_resource_listing_without_vm2_retry() {
    let server = MockServer::start();
    let legacy = mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    let listed = mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params(
            "0x1",
            true,
            0,
            20,
            None,
            &[String::from("0x1::account::Account")],
        ),
        json!({
            "resources": [
                {
                    "name": "0x1::account::Account",
                    "value": {
                        "json": {
                            "sequence_number": "11"
                        }
                    }
                }
            ]
        }),
    );
    let vm2 = mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params(
            "0x1",
            true,
            0,
            20,
            None,
            &[String::from("0x1::account::Account")],
        ),
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 99
                    }
                }
            }
        }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let state = client
        .get_account_state("0x1")
        .await
        .expect("legacy array resource listing should be enough")
        .expect("account state should be synthesized");

    assert_eq!(state.get("sequence_number"), Some(&json!(11)));
    assert_eq!(legacy.hits(), 1);
    assert_eq!(listed.hits(), 1);
    assert_eq!(vm2.hits(), 0);
}

#[tokio::test]
async fn get_account_state_propagates_vm2_errors_after_legacy_null() {
    let server = MockServer::start();
    mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"state2.list_resource\"")
            .body_contains(&format!(
                "\"params\":{}",
                serde_json::to_string(&super::vm2_list_resources_params(
                    "0x1",
                    true,
                    0,
                    32,
                    None,
                    &[]
                ))
                .expect("params should serialize")
            ));
        then.status(503).body("vm2 unavailable");
    });

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let error = client
        .get_account_state("0x1")
        .await
        .expect_err("vm2 fallback errors should propagate");

    assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
}

#[tokio::test]
async fn get_account_state_rejects_malformed_vm2_resource_envelope() {
    let server = MockServer::start();
    mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params("0x1", true, 0, 32, None, &[]),
        json!({ "unexpected": {} }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let error = client
        .get_account_state("0x1")
        .await
        .expect_err("malformed vm2 envelopes should propagate as rpc failures");

    assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
}

#[tokio::test]
async fn list_resources_propagates_vm2_errors_after_legacy_empty() {
    let server = MockServer::start();
    mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({ "resources": {} }),
    );
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"state2.list_resource\"")
            .body_contains(&format!(
                "\"params\":{}",
                serde_json::to_string(&super::vm2_list_resources_params(
                    "0x1",
                    true,
                    0,
                    20,
                    None,
                    &[]
                ))
                .expect("params should serialize")
            ));
        then.status(503).body("vm2 unavailable");
    });

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let error = client
        .list_resources("0x1", true, 0, 20, None, &[])
        .await
        .expect_err("vm2 fallback errors should propagate");

    assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
}

#[tokio::test]
async fn list_resources_rejects_malformed_vm2_resource_envelope() {
    let server = MockServer::start();
    mock_json_rpc_result_with_params(
        &server,
        "state.list_resource",
        super::legacy_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({ "resources": {} }),
    );
    mock_json_rpc_result_with_params(
        &server,
        "state2.list_resource",
        super::vm2_list_resources_params("0x1", true, 0, 20, None, &[]),
        json!({ "unexpected": {} }),
    );

    let client = NodeRpcClient::new(&runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .expect("rpc client should build");
    let error = client
        .list_resources("0x1", true, 0, 20, None, &[])
        .await
        .expect_err("malformed vm2 resource envelopes should fail closed");

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
    let mut config = runtime_config(&server, Mode::ReadOnly, VmProfile::Vm2Only, true);
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
