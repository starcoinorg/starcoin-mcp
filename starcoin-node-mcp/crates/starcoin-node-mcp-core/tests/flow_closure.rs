use std::{path::PathBuf, time::Duration};

use httpmock::{Mock, MockServer, prelude::*};
use serde_json::{Value, json};
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_types::{
    GetAccountOverviewInput, GetTransactionInput, ListResourcesInput, Mode, PrepareTransferInput,
    RuntimeConfig, SharedErrorCode, SimulateRawTransactionInput, SimulationStatus, SubmissionState,
    SubmitSignedTransactionInput, VmProfile,
};
use starcoin_vm2_crypto::ed25519::genesis_key_pair;
use starcoin_vm2_vm_types::transaction::RawUserTransaction;
use url::Url;

#[tokio::test]
async fn submit_policy_requires_local_simulation_attestation() {
    let server = MockServer::start();
    mock_json_rpc_result(&server, "node.status", json!(true));
    mock_json_rpc_result(&server, "chain.info", sample_chain_info());
    mock_json_rpc_result(&server, "node.info", sample_node_info());
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
    mock_json_rpc_error(&server, "chain.get_events", -32601, "method not found");
    mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({ "sequence_number": "0" }),
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
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    mock_json_rpc_result(&server, "txpool.next_sequence_number2", json!("0"));
    mock_json_rpc_result(
        &server,
        "contract2.dry_run_raw",
        json!({ "status": "Executed", "gas_used": "7", "events": [] }),
    );
    let submit = mock_json_rpc_result(&server, "txpool.submit_hex_transaction2", json!("0xabc"));

    let app = AppContext::bootstrap(runtime_config(&server, Mode::Transaction, false))
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
    mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({ "sequence_number": "7" }),
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
    mock_json_rpc_error(
        &server,
        "txpool.next_sequence_number2",
        -32601,
        "method not found",
    );
    mock_json_rpc_error(
        &server,
        "txpool.next_sequence_number",
        -32601,
        "method not found",
    );

    let app = AppContext::bootstrap(runtime_config(&server, Mode::ReadOnly, true))
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
async fn get_transaction_skips_event_fetch_until_confirmation() {
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
    let txn_events = mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
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

    let app = AppContext::bootstrap(runtime_config(&server, Mode::ReadOnly, true))
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
    mock_json_rpc_result(&server, "node.info", sample_node_info());
    mock_json_rpc_result(&server, "chain.info", sample_chain_info());
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"node.peers\"");
        then.status(503).body("peers unavailable");
    });
    mock_json_rpc_result(&server, "sync.status", json!({ "state": "idle" }));
    mock_json_rpc_result(&server, "txpool.state", json!({ "txn_count": 0 }));
    mock_json_rpc_result(&server, "node.status", json!(true));
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

    let app = AppContext::bootstrap(runtime_config(&server, Mode::ReadOnly, true))
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
    mock_json_rpc_result(&server, "node.status", json!(true));
    mock_json_rpc_result(&server, "chain.info", sample_chain_info());
    mock_json_rpc_result(&server, "node.info", sample_node_info());
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
    mock_json_rpc_result(&server, "state.list_resource", json!({ "resources": {} }));
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

    let app = AppContext::bootstrap(runtime_config(&server, Mode::ReadOnly, true))
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
    mock_json_rpc_result(&server, "node.status", json!(true));
    mock_json_rpc_result(&server, "chain.info", sample_chain_info());
    mock_json_rpc_result(&server, "node.info", sample_node_info());
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_transaction_info2", Value::Null);
    mock_json_rpc_result(&server, "chain.get_events_by_txn_hash2", json!([]));
    mock_json_rpc_error(&server, "chain.get_events", -32601, "method not found");
    mock_json_rpc_result(
        &server,
        "state.get_account_state",
        json!({ "sequence_number": "4" }),
    );
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"txpool.next_sequence_number2\"")
            .body_contains("\"0x00000000000000000000000000000000\"");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0",
                })
                .to_string(),
            );
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"txpool.next_sequence_number2\"")
            .body_contains("\"0x1\"");
        then.status(503).body("txpool unavailable");
    });
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
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    server.mock(|when, then| {
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
    let app = AppContext::bootstrap(runtime_config(&server, Mode::Transaction, true))
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

fn runtime_config(
    server: &MockServer,
    mode: Mode,
    allow_submit_without_prior_simulation: bool,
) -> RuntimeConfig {
    RuntimeConfig {
        rpc_endpoint_url: Url::parse(&server.url("/")).expect("mock url should parse"),
        mode,
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
