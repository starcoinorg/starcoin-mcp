use httpmock::{MockServer, prelude::*};
use serde_json::{Value, json};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_test_support::{
    mock_abi_methods_not_found, mock_block_lookup_probe, mock_json_rpc_result,
    mock_method_not_found, mock_probe_metadata, mock_submit_probe_invalid_params,
    mock_transaction_event_methods_not_found, mock_transaction_info_methods_not_found,
    mock_transaction_lookup_probe, mock_txpool_sequence_probe, mock_view_methods_not_found,
    runtime_config, sample_node_info,
};
use starcoin_node_mcp_types::{
    GetAccountOverviewInput, GetTransactionInput, ListResourcesInput, Mode, PrepareTransferInput,
    SharedErrorCode, SimulateRawTransactionInput, SimulationStatus, SubmissionState,
    SubmitSignedTransactionInput, VmProfile, WatchTransactionInput,
};
use starcoin_vm2_crypto::ed25519::genesis_key_pair;
use starcoin_vm2_vm_types::transaction::RawUserTransaction;

#[tokio::test]
async fn submit_policy_requires_local_simulation_attestation() {
    let server = MockServer::start();
    mock_transaction_bootstrap(&server);
    mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({ "sequence_number": "0" }),
    );
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    mock_txpool_sequence_probe(&server, "txpool.next_sequence_number2", json!("0"));
    mock_json_rpc_result(
        &server,
        "contract2.dry_run_raw",
        json!({ "status": "Executed", "gas_used": "7", "events": [] }),
    );
    let submit = mock_json_rpc_result(&server, "txpool.submit_hex_transaction2", json!("0xabc"));

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::Transaction,
        VmProfile::Auto,
        false,
    ))
    .await
    .expect("transaction app should bootstrap");

    let prepared = app
        .prepare_transfer(PrepareTransferInput {
            sender: "0x1".to_owned(),
            sender_public_key: None,
            receiver: "0x2".to_owned(),
            amount: "1".to_owned(),
            token_code: None,
            sequence_number: None,
            max_gas_amount: None,
            gas_unit_price: None,
            expiration_time_secs: None,
            gas_token: None,
        })
        .await
        .expect("prepare should succeed");
    assert_eq!(
        prepared.simulation_status,
        SimulationStatus::SkippedMissingPublicKey
    );

    let raw_txn: RawUserTransaction = bcs_ext::from_bytes(
        &hex::decode(&prepared.raw_txn_bcs_hex).expect("raw hex should decode"),
    )
    .expect("raw transaction should decode");
    let (private_key, public_key) = genesis_key_pair();
    let public_key_hex = format!("0x{}", hex::encode(public_key.to_bytes()));
    let signed_txn = raw_txn
        .sign(&private_key, public_key)
        .expect("transaction should sign")
        .into_inner();
    let signed_txn_bcs_hex = format!(
        "0x{}",
        hex::encode(bcs_ext::to_bytes(&signed_txn).expect("signed txn should encode"))
    );

    let error = app
        .submit_signed_transaction(SubmitSignedTransactionInput {
            signed_txn_bcs_hex: signed_txn_bcs_hex.clone(),
            prepared_chain_context: prepared.chain_context.clone(),
            blocking: false,
            timeout_seconds: None,
            min_confirmed_blocks: None,
        })
        .await
        .expect_err("submit should be rejected before txpool when simulation is required");
    assert_eq!(error.code, SharedErrorCode::PermissionDenied);
    assert_eq!(
        submit.hits(),
        1,
        "only startup probe should hit txpool.submit"
    );

    let simulation = app
        .simulate_raw_transaction(SimulateRawTransactionInput {
            raw_txn_bcs_hex: prepared.raw_txn_bcs_hex.clone(),
            sender_public_key: public_key_hex,
        })
        .await
        .expect("explicit simulation should succeed");
    assert!(simulation.executed);

    let submission = app
        .submit_signed_transaction(SubmitSignedTransactionInput {
            signed_txn_bcs_hex,
            prepared_chain_context: prepared.chain_context,
            blocking: false,
            timeout_seconds: None,
            min_confirmed_blocks: None,
        })
        .await
        .expect("submit should succeed after simulation");
    assert_eq!(submission.submission_state, SubmissionState::Accepted);
    assert_eq!(
        submission.prepared_simulation_status,
        Some(SimulationStatus::Performed)
    );
    assert_eq!(
        submit.hits(),
        2,
        "second txpool.submit should be the real submission"
    );
}

#[tokio::test]
async fn account_overview_degrades_when_sequence_hint_is_unavailable() {
    let server = MockServer::start();
    mock_read_only_bootstrap(&server);
    mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    mock_json_rpc_result(
        &server,
        "state2.list_resource",
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
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_method_not_found(&server, "txpool.next_sequence_number2");
    mock_method_not_found(&server, "txpool.next_sequence_number");

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("read_only app should bootstrap");

    let overview = app
        .get_account_overview(GetAccountOverviewInput {
            address: "0x1".to_owned(),
            include_resources: false,
            include_modules: false,
            resource_limit: None,
            module_limit: None,
        })
        .await
        .expect("account overview should degrade cleanly");
    assert!(overview.onchain_exists);
    assert_eq!(overview.sequence_number, Some(7));
    assert_eq!(overview.next_sequence_number_hint, None);
}

#[tokio::test]
async fn account_overview_uses_primary_store_balance_when_resource_page_excludes_it() {
    let server = MockServer::start();
    mock_read_only_bootstrap(&server);
    mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
    mock_json_rpc_result(
        &server,
        "state2.list_resource",
        json!({
            "resources": {
                "0x1::account::Account": {
                    "json": {
                        "sequence_number": 7
                    }
                },
                "0x1::coin::CoinStore<0x00000000000000000000000000000001::starcoin_coin::STC>": {
                    "json": {
                        "coin": { "value": 0 }
                    }
                }
            }
        }),
    );
    mock_json_rpc_result(
        &server,
        "state2.get_resource",
        json!({
            "raw": "0x01",
            "json": {
                "balance": 42
            }
        }),
    );
    mock_method_not_found(&server, "state.list_code");
    mock_method_not_found(&server, "txpool.next_sequence_number2");
    mock_method_not_found(&server, "txpool.next_sequence_number");

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("read_only app should bootstrap");

    let overview = app
        .get_account_overview(GetAccountOverviewInput {
            address: "0x1".to_owned(),
            include_resources: true,
            include_modules: false,
            resource_limit: Some(2),
            module_limit: None,
        })
        .await
        .expect("account overview should use the dedicated primary store balance");

    assert_eq!(overview.sequence_number, Some(7));
    assert_eq!(overview.balances.len(), 1);
    assert_eq!(
        overview.balances[0].get("name").and_then(Value::as_str),
        Some("0x00000000000000000000000000000001::fungible_asset::FungibleStore")
    );
    assert_eq!(
        overview.accepted_tokens,
        vec!["0x00000000000000000000000000000001::starcoin_coin::STC".to_owned()]
    );
}

#[tokio::test]
async fn watch_transaction_times_out_when_confirmation_depth_is_not_reached() {
    let server = MockServer::start();
    mock_probe_metadata_with_head(&server, 42);
    mock_block_lookup_probe(&server, Value::Null);
    mock_method_not_found(&server, "chain.get_blocks_by_number");
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_json_rpc_result(
        &server,
        "chain.get_transaction_info2",
        json!({
            "block_number": 42,
            "status": "Executed",
            "gas_used": "7",
        }),
    );
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
    .expect("watch app should bootstrap");

    let watch = app
        .watch_transaction(WatchTransactionInput {
            txn_hash: "0x1".to_owned(),
            timeout_seconds: Some(0),
            poll_interval_seconds: Some(0),
            min_confirmed_blocks: Some(2),
        })
        .await
        .expect("watch should return a bounded timeout result");

    assert!(watch.found);
    assert!(!watch.confirmed);
    assert_eq!(watch.effective_min_confirmed_blocks, 2);
    assert_eq!(watch.confirmed_blocks, Some(1));
    assert_eq!(watch.inclusion_block_number, Some(42));
    assert!(watch.status_summary.confirmed);
}

#[tokio::test]
async fn blocking_submit_uses_watch_confirmation_depth_defaults() {
    let server = MockServer::start();
    mock_probe_metadata_with_head(&server, 43);
    mock_block_lookup_probe(&server, Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_json_rpc_result(
        &server,
        "chain.get_transaction_info2",
        json!({
            "block_number": 42,
            "status": "Executed",
            "gas_used": "7",
        }),
    );
    mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
    mock_method_not_found(&server, "chain.get_events");
    mock_abi_methods_not_found(&server);
    mock_view_methods_not_found(&server);
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    mock_json_rpc_result(&server, "txpool.next_sequence_number2", json!("0"));
    mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
    mock_method_not_found(&server, "state.list_code");
    mock_submit_probe_invalid_params(&server, "txpool.submit_hex_transaction2");
    mock_json_rpc_result(
        &server,
        "contract2.dry_run_raw",
        json!({ "status": "Executed" }),
    );
    mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({ "sequence_number": "0" }),
    );
    mock_json_rpc_result(
        &server,
        "txpool.submit_hex_transaction2",
        json!("0xaccepted"),
    );

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::Transaction,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("transaction app should bootstrap");
    let prepared = app
        .prepare_transfer(PrepareTransferInput {
            sender: "0x1".to_owned(),
            sender_public_key: None,
            receiver: "0x2".to_owned(),
            amount: "1".to_owned(),
            token_code: None,
            sequence_number: None,
            max_gas_amount: None,
            gas_unit_price: None,
            expiration_time_secs: None,
            gas_token: None,
        })
        .await
        .expect("prepare should succeed");
    let raw_txn: RawUserTransaction = bcs_ext::from_bytes(
        &hex::decode(&prepared.raw_txn_bcs_hex).expect("raw hex should decode"),
    )
    .expect("raw transaction should decode");
    let (private_key, public_key) = genesis_key_pair();
    let signed_txn = raw_txn
        .sign(&private_key, public_key)
        .expect("transaction should sign")
        .into_inner();
    let signed_txn_bcs_hex = format!(
        "0x{}",
        hex::encode(bcs_ext::to_bytes(&signed_txn).expect("signed txn should encode"))
    );

    let submission = app
        .submit_signed_transaction(SubmitSignedTransactionInput {
            signed_txn_bcs_hex,
            prepared_chain_context: prepared.chain_context,
            blocking: true,
            timeout_seconds: Some(0),
            min_confirmed_blocks: None,
        })
        .await
        .expect("blocking submit should succeed");

    let watch = submission
        .watch_result
        .expect("blocking submit should include watch_result");
    assert_eq!(submission.submission_state, SubmissionState::Accepted);
    assert!(watch.confirmed);
    assert_eq!(watch.effective_min_confirmed_blocks, 2);
    assert_eq!(watch.confirmed_blocks, Some(2));
    assert_eq!(watch.inclusion_block_number, Some(42));
}

#[tokio::test]
async fn get_transaction_skips_event_fetch_until_confirmation() {
    let server = MockServer::start();
    mock_read_only_bootstrap(&server);
    mock_json_rpc_result(
        &server,
        "chain.get_transaction2",
        json!({ "status": "Pending" }),
    );
    mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
    let txn_events = mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
    mock_method_not_found(&server, "state.get_account_state");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("read_only app should bootstrap");
    let hits_after_bootstrap = txn_events.hits();

    let transaction = app
        .get_transaction(GetTransactionInput {
            txn_hash: "0x1".to_owned(),
            include_events: true,
            decode: true,
        })
        .await
        .expect("transaction lookup should succeed");
    assert!(transaction.status_summary.found);
    assert!(!transaction.status_summary.confirmed);
    assert!(transaction.events.is_empty());
    assert_eq!(
        txn_events.hits(),
        hits_after_bootstrap,
        "unconfirmed transaction lookups should not fetch events again",
    );
}

#[tokio::test]
async fn node_health_skips_zero_peer_warning_when_peer_rpc_is_unavailable() {
    let server = MockServer::start();
    mock_probe_metadata(&server);
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"node.peers\"");
        then.status(503).body("peers unavailable");
    });
    mock_json_rpc_result(&server, "sync.status", json!({ "state": "idle" }));
    mock_json_rpc_result(&server, "txpool.state", json!({ "txn_count": 0 }));
    mock_read_only_query_surface(&server);
    mock_method_not_found(&server, "state.get_account_state");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");

    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::ReadOnly,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("read_only app should bootstrap");
    let health = app.node_health().await.expect("node health should degrade");

    assert!(
        health
            .warnings
            .iter()
            .any(|warning| warning.contains("node.peers unavailable"))
    );
    assert!(
        !health
            .warnings
            .iter()
            .any(|warning| warning.contains("zero connected peers"))
    );
}

#[tokio::test]
async fn historical_resource_queries_fail_when_block_state_root_is_missing() {
    let server = MockServer::start();
    mock_probe_metadata(&server);
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
                    "result": Value::Null,
                })
                .to_string(),
            );
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"chain.get_block_by_number\"")
            .body_contains("[7,");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "header": {
                            "number": "7",
                            "block_hash": "0x7"
                        }
                    },
                })
                .to_string(),
            );
    });
    mock_method_not_found(&server, "chain.get_blocks_by_number");
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_transaction_info_methods_not_found(&server);
    mock_transaction_event_methods_not_found(&server);
    mock_method_not_found(&server, "chain.get_events");
    mock_method_not_found(&server, "state.get_account_state");
    mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
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
    .expect("read_only app should bootstrap");
    let error = app
        .list_resources(ListResourcesInput {
            address: "0x1".to_owned(),
            block_number: Some(7),
            decode: true,
            start_index: None,
            max_size: None,
            resource_type: None,
        })
        .await
        .expect_err("historical resource lookup should fail without block state_root");

    assert_eq!(error.code, SharedErrorCode::RpcUnavailable);
    assert!(
        error.message.contains("missing header.state_root"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn prepare_transfer_rejects_past_expiration_and_normalizes_nested_dry_run_events() {
    let server = MockServer::start();
    mock_transaction_bootstrap(&server);
    mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({ "sequence_number": "4" }),
    );
    mock_txpool_sequence_probe(&server, "txpool.next_sequence_number2", json!("0"));
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"txpool.next_sequence_number2\"")
            .body_contains("\"0x1\"");
        then.status(503).body("txpool unavailable");
    });
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    mock_submit_probe_invalid_params(&server, "txpool.submit_hex_transaction2");
    let (private_key, public_key) = genesis_key_pair();
    let public_key_hex = format!("0x{}", hex::encode(public_key.to_bytes()));
    mock_json_rpc_result(
        &server,
        "contract2.dry_run_raw",
        json!({
            "txn_output": {
                "status": "Executed",
                "gas_used": "7",
                "events": [{ "type_tag": "0x1::Test::Event" }],
                "write_set": [],
            }
        }),
    );
    let app = AppContext::bootstrap(runtime_config(
        &server,
        Mode::Transaction,
        VmProfile::Auto,
        true,
    ))
    .await
    .expect("transaction app should bootstrap");

    let past_expiration_error = app
        .prepare_transfer(PrepareTransferInput {
            sender: "0x1".to_owned(),
            sender_public_key: None,
            receiver: "0x2".to_owned(),
            amount: "1".to_owned(),
            token_code: None,
            sequence_number: None,
            max_gas_amount: None,
            gas_unit_price: None,
            expiration_time_secs: Some(119),
            gas_token: None,
        })
        .await
        .expect_err("past expirations should be rejected");
    assert_eq!(
        past_expiration_error.code,
        SharedErrorCode::TransactionExpired
    );

    let prepared = app
        .prepare_transfer(PrepareTransferInput {
            sender: "0x1".to_owned(),
            sender_public_key: Some(public_key_hex),
            receiver: "0x2".to_owned(),
            amount: "1".to_owned(),
            token_code: None,
            sequence_number: None,
            max_gas_amount: None,
            gas_unit_price: None,
            expiration_time_secs: None,
            gas_token: None,
        })
        .await
        .expect("prepare should degrade sequence lookup and normalize nested events");

    let simulation = prepared
        .simulation
        .expect("sender_public_key should force dry run during prepare");
    assert_eq!(
        prepared.sequence_number_source,
        starcoin_node_mcp_types::SequenceNumberSource::Onchain
    );
    assert_eq!(simulation.events.len(), 1);
    drop(private_key);
}

fn mock_read_only_bootstrap(server: &MockServer) {
    mock_probe_metadata(server);
    mock_read_only_query_surface(server);
}

fn mock_probe_metadata_with_head(server: &MockServer, head_block_number: u64) {
    mock_json_rpc_result(server, "node.status", json!(true));
    mock_json_rpc_result(
        server,
        "chain.info",
        json!({
            "chain_id": 254,
            "genesis_hash": "0x1",
            "head": {
                "number": head_block_number,
                "block_hash": "0x2",
                "state_root": "0x3",
                "timestamp": 100,
            }
        }),
    );
    mock_json_rpc_result(server, "node.info", sample_node_info());
}

fn mock_read_only_query_surface(server: &MockServer) {
    mock_block_lookup_probe(server, Value::Null);
    mock_method_not_found(server, "chain.get_blocks_by_number");
    mock_transaction_lookup_probe(server, "chain.get_transaction2", Value::Null);
    mock_transaction_info_methods_not_found(server);
    mock_transaction_event_methods_not_found(server);
    mock_method_not_found(server, "chain.get_events");
    mock_abi_methods_not_found(server);
    mock_view_methods_not_found(server);
}

fn mock_transaction_bootstrap(server: &MockServer) {
    mock_probe_metadata(server);
    mock_block_lookup_probe(server, Value::Null);
    mock_json_rpc_result(server, "chain.get_blocks_by_number", json!([]));
    mock_transaction_lookup_probe(server, "chain.get_transaction2", Value::Null);
    mock_json_rpc_result(server, "chain.get_transaction_info2", Value::Null);
    mock_json_rpc_result(server, "chain.get_events_by_txn_hash2", json!([]));
    mock_method_not_found(server, "chain.get_events");
    mock_abi_methods_not_found(server);
    mock_view_methods_not_found(server);
}
