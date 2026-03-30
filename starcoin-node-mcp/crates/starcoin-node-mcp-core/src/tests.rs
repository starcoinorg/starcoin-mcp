use super::{
    AppContext, CachedProbe, SignedUserTransaction, accepted_submission_output,
    effective_submit_timeout_seconds, enforce_transaction_head_lag, extract_balances_and_tokens,
    extract_chain_context, extract_optional_string, is_terminal_watch_status,
    status_summary_from_parts,
    submission_unknown_output, validate_chain_identity, validate_signed_transaction_submission,
    validate_transaction_probe,
};
use httpmock::prelude::*;
use serde_json::{Value, json};
use starcoin_node_mcp_rpc::NodeRpcClient;
use starcoin_node_mcp_test_support::{
    mock_abi_methods_not_found, mock_json_rpc_error, mock_json_rpc_result, mock_method_not_found,
    mock_probe_metadata, mock_submit_probe_invalid_params,
    mock_transaction_event_methods_not_found, mock_transaction_info_methods_not_found,
    mock_view_methods_not_found, runtime_config_with_endpoint,
};
use starcoin_node_mcp_types::{
    ChainContext, EffectiveProbe, Mode, RuntimeConfig, SimulationStatus, SubmissionState,
    SubmitSignedTransactionInput, VmProfile,
};
use starcoin_vm2_crypto::ed25519::genesis_key_pair;
use starcoin_vm2_vm_types::{
    account_address::AccountAddress,
    on_chain_resource::ChainId,
    transaction::{Script, TransactionPayload},
};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, Semaphore};

#[test]
fn status_summary_marks_confirmation_when_info_exists() {
    let summary = status_summary_from_parts(
        Some(&json!({"status": "Pending"})),
        Some(&json!({"status": "Executed", "gas_used": "42"})),
    );
    assert!(summary.found);
    assert!(summary.confirmed);
    assert_eq!(summary.gas_used, Some(42));
}

#[test]
fn extract_chain_context_handles_builtin_network_shape() {
    let context = extract_chain_context(
        &json!({
            "net": { "Builtin": "Barnard" },
            "now_seconds": 100,
        }),
        &json!({
            "chain_id": 251,
            "genesis_hash": "0x1",
            "head": {
                "block_hash": "0x2",
                "number": "42",
                "state_root": "0x3",
            }
        }),
    )
    .expect("builtin network should parse");
    assert_eq!(context.network, "barnard");
    assert_eq!(context.chain_id, 251);
}

#[test]
fn validate_chain_identity_accepts_case_insensitive_network_names() {
    let config = sample_runtime_config();
    validate_chain_identity(
        &RuntimeConfig {
            expected_network: Some("main".to_owned()),
            ..config
        },
        &json!({
            "net": { "Builtin": "Main" },
            "now_seconds": 100,
        }),
        &json!({
            "chain_id": 254,
            "genesis_hash": "0x1",
            "head": {
                "block_hash": "0x2",
                "number": "1",
            }
        }),
    )
    .expect("builtin network names should compare case-insensitively");
}

#[test]
fn extract_optional_string_rejects_non_string_values() {
    let value = json!({
        "number": 1,
        "array": [],
        "object": {},
        "null_value": null,
        "string": "ok"
    });
    assert_eq!(
        extract_optional_string(&value, &["string"]),
        Some("ok".to_owned())
    );
    assert_eq!(extract_optional_string(&value, &["number"]), None);
    assert_eq!(extract_optional_string(&value, &["array"]), None);
    assert_eq!(extract_optional_string(&value, &["object"]), None);
    assert_eq!(extract_optional_string(&value, &["null_value"]), None);
}

#[test]
fn extract_balances_and_tokens_recognizes_coin_store_resources() {
    let resources = vec![
        json!({
            "name": "0x00000000000000000000000000000001::account::Account",
            "value": {}
        }),
        json!({
            "name": "0x00000000000000000000000000000001::coin::CoinStore<0x00000000000000000000000000000001::starcoin_coin::STC>",
            "value": {
                "json": {
                    "coin": { "value": 7 }
                }
            }
        }),
        json!({
            "name": "0x00000000000000000000000000000001::fungible_asset::FungibleStore",
            "value": {
                "json": {
                    "balance": 42
                }
            }
        }),
    ];
    let mut balances = Vec::new();
    let mut accepted_tokens = Vec::new();

    extract_balances_and_tokens(&resources, &mut balances, &mut accepted_tokens);

    assert_eq!(balances.len(), 1);
    assert_eq!(
        balances[0].get("name").and_then(Value::as_str),
        Some("0x00000000000000000000000000000001::fungible_asset::FungibleStore")
    );
    assert_eq!(
        accepted_tokens,
        vec!["0x00000000000000000000000000000001::starcoin_coin::STC".to_owned()]
    );
}

#[test]
fn watch_only_terminates_on_confirmation() {
    assert!(!is_terminal_watch_status(&status_summary_from_parts(
        Some(&json!({"status": "Pending"})),
        None,
    )));
    assert!(is_terminal_watch_status(&status_summary_from_parts(
        Some(&json!({"status": "Pending"})),
        Some(&json!({"status": "Executed"})),
    )));
}

#[test]
fn transaction_probe_requires_submission_and_dry_run() {
    validate_transaction_probe(&sample_probe()).expect("fully capable probe should pass");
    let missing_submit = EffectiveProbe {
        supports_transaction_submission: false,
        ..sample_probe()
    };
    assert_eq!(
        validate_transaction_probe(&missing_submit)
            .expect_err("missing submit capability should fail")
            .code,
        starcoin_node_mcp_types::SharedErrorCode::UnsupportedOperation
    );
}

#[test]
fn submit_timeout_is_clamped_when_blocking() {
    assert_eq!(
        effective_submit_timeout_seconds(
            true,
            Some(90),
            Duration::from_secs(30),
            Duration::from_secs(60),
        ),
        Some(60)
    );
    assert_eq!(
        effective_submit_timeout_seconds(
            false,
            Some(90),
            Duration::from_secs(30),
            Duration::from_secs(60),
        ),
        None
    );
}

#[test]
fn head_lag_policy_fails_closed_above_threshold() {
    enforce_transaction_head_lag(120, 90, Duration::from_secs(60))
        .expect("healthy lag should pass");
    assert_eq!(
        enforce_transaction_head_lag(200, 100, Duration::from_secs(30))
            .expect_err("lag above threshold should fail")
            .code,
        starcoin_node_mcp_types::SharedErrorCode::RpcUnavailable
    );
}

#[test]
fn resolve_expiration_rejects_past_requested_timestamps() {
    let app = sample_app_context();
    let error = app
        .resolve_expiration(120, Some(119))
        .expect_err("past expiration should fail before preparation");
    assert_eq!(
        error.code,
        starcoin_node_mcp_types::SharedErrorCode::TransactionExpired
    );
}

#[test]
fn validate_signed_submission_accepts_matching_chain_context() {
    let signed_txn = sample_signed_transaction(254, 7);
    let prepared = sample_chain_context(254, "main", "0x1");
    let current = sample_chain_context(254, "Main", "0x1");
    validate_signed_transaction_submission(&signed_txn, &prepared, &current)
        .expect("matching signed and prepared chain contexts should pass");
}

#[test]
fn validate_signed_submission_rejects_chain_mismatch() {
    let signed_txn = sample_signed_transaction(254, 7);
    let prepared = sample_chain_context(254, "main", "0x1");
    let current = sample_chain_context(253, "barnard", "0x2");
    let error = validate_signed_transaction_submission(&signed_txn, &prepared, &current)
        .expect_err("mismatched endpoint identity should fail");
    assert_eq!(
        error.code,
        starcoin_node_mcp_types::SharedErrorCode::InvalidChainContext
    );
}

#[test]
fn submit_result_helpers_preserve_policy_shape() {
    let accepted = accepted_submission_output(
        "0x1".to_owned(),
        true,
        Some(SimulationStatus::Performed),
        Some(5),
        None,
    );
    assert_eq!(
        accepted.submission_state,
        starcoin_node_mcp_types::SubmissionState::Accepted
    );
    assert!(accepted.submitted);
    assert_eq!(
        accepted.prepared_simulation_status,
        Some(SimulationStatus::Performed)
    );

    let unknown = submission_unknown_output(
        "0x2".to_owned(),
        Some(SimulationStatus::SkippedMissingPublicKey),
        Some(9),
    );
    assert_eq!(unknown.error_code.as_deref(), Some("submission_unknown"));
    assert_eq!(
        unknown.next_action,
        starcoin_node_mcp_types::SubmissionNextAction::ReconcileByTxnHash
    );
}

#[tokio::test]
async fn unresolved_submission_entries_expire_from_local_policy_cache() {
    let app = sample_app_context();
    assert!(!app.has_unresolved_submission("0xabc").await);

    app.record_unresolved_submission("0xabc").await;
    assert!(app.has_unresolved_submission("0xabc").await);

    {
        let mut unresolved = app.unresolved_submissions.write().await;
        unresolved
            .get_mut("0xabc")
            .expect("entry should exist")
            .recorded_at = Instant::now()
            - (app.config.max_expiration_ttl
                + app.config.max_watch_timeout
                + Duration::from_secs(1));
    }

    assert!(!app.has_unresolved_submission("0xabc").await);
}

#[tokio::test]
async fn submit_unknown_blocks_blind_resubmission_before_second_txpool_call() {
    let server = MockServer::start();
    let signed_txn = sample_signed_transaction(254, 0);
    let signed_txn_bcs_hex = format!(
        "0x{}",
        hex::encode(bcs_ext::to_bytes(&signed_txn).expect("sample tx should serialize"))
    );
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_transaction_info_methods_not_found(&server);
    mock_transaction_event_methods_not_found(&server);
    mock_method_not_found(&server, "chain.get_events");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_abi_methods_not_found(&server);
    mock_view_methods_not_found(&server);
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    mock_json_rpc_result(&server, "txpool.next_sequence_number2", json!("0"));
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
    let submit = server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"txpool.submit_hex_transaction2\"")
            .body_contains(&signed_txn_bcs_hex);
        then.status(503).body("submit unavailable");
    });

    let app = AppContext::bootstrap(sample_runtime_config_with_endpoint(&server.url("/")))
        .await
        .expect("bootstrap should succeed");
    let hits_after_bootstrap = submit.hits();
    let input = SubmitSignedTransactionInput {
        signed_txn_bcs_hex,
        prepared_chain_context: sample_chain_context(254, "main", "0x1"),
        blocking: false,
        timeout_seconds: None,
    };

    let first = app
        .submit_signed_transaction(input.clone())
        .await
        .expect("first submission should return an unknown state");
    assert_eq!(first.submission_state, SubmissionState::Unknown);
    assert_eq!(first.error_code.as_deref(), Some("submission_unknown"));
    assert_eq!(submit.hits(), hits_after_bootstrap + 1);

    let second = app
        .submit_signed_transaction(input)
        .await
        .expect("second submission should be blocked locally");
    assert_eq!(second.submission_state, SubmissionState::Unknown);
    assert_eq!(second.error_code.as_deref(), Some("submission_unknown"));
    assert_eq!(submit.hits(), hits_after_bootstrap + 1);
}

#[tokio::test]
async fn submit_signed_transaction_degrades_when_sequence_lookup_returns_no_sender_state() {
    let server = MockServer::start();
    let signed_txn = sample_signed_transaction(254, 1);
    let signed_txn_bcs_hex = format!(
        "0x{}",
        hex::encode(bcs_ext::to_bytes(&signed_txn).expect("sample tx should serialize"))
    );
    mock_probe_metadata(&server);
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
    mock_json_rpc_result(&server, "chain.get_transaction2", Value::Null);
    mock_transaction_info_methods_not_found(&server);
    mock_transaction_event_methods_not_found(&server);
    mock_method_not_found(&server, "chain.get_events");
    mock_method_not_found(&server, "state.list_resource");
    mock_method_not_found(&server, "state.list_code");
    mock_abi_methods_not_found(&server);
    mock_view_methods_not_found(&server);
    mock_json_rpc_result(&server, "txpool.gas_price", json!("1"));
    mock_json_rpc_result(&server, "txpool.next_sequence_number2", Value::Null);
    mock_submit_probe_invalid_params(&server, "txpool.submit_hex_transaction2");
    mock_json_rpc_result(
        &server,
        "contract2.dry_run_raw",
        json!({ "status": "Executed" }),
    );
    mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    let submit = server.mock(|when, then| {
        when.method(POST)
            .path("/")
            .body_contains("\"method\":\"txpool.submit_hex_transaction2\"")
            .body_contains(&signed_txn_bcs_hex);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": signed_txn.id().to_string(),
                })
                .to_string(),
            );
    });

    let app = AppContext::bootstrap(sample_runtime_config_with_endpoint(&server.url("/")))
        .await
        .expect("bootstrap should succeed");
    let result = app
        .submit_signed_transaction(SubmitSignedTransactionInput {
            signed_txn_bcs_hex,
            prepared_chain_context: sample_chain_context(254, "main", "0x1"),
            blocking: false,
            timeout_seconds: None,
        })
        .await
        .expect("submit should degrade stale-check when sequence sources are unavailable");

    assert_eq!(result.submission_state, SubmissionState::Accepted);
    assert!(result.submitted);
    assert_eq!(submit.hits(), 1);
}

#[tokio::test]
async fn resolve_sequence_number_degrades_when_txpool_sequence_hint_is_unavailable() {
    let server = MockServer::start();
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
    mock_json_rpc_result(&server, "state.get_account_state", Value::Null);
    mock_json_rpc_result(
        &server,
        "state2.list_resource",
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
    let config = sample_runtime_config_with_endpoint(&server.url("/"));
    let app = AppContext {
        rpc: NodeRpcClient::new(&config).expect("rpc client should build"),
        watch_permits: Arc::new(Semaphore::new(config.max_concurrent_watch_requests)),
        expensive_permits: Arc::new(Semaphore::new(config.max_inflight_expensive_requests)),
        startup_probe: sample_probe(),
        transaction_probe: Arc::new(RwLock::new(Some(CachedProbe {
            probe: sample_probe(),
            observed_at: Instant::now(),
        }))),
        prepared_transactions: Arc::new(RwLock::new(HashMap::new())),
        unresolved_submissions: Arc::new(RwLock::new(HashMap::new())),
        config,
    };

    let (sequence, source) = app
        .resolve_sequence_number("0x1", None)
        .await
        .expect("on-chain sequence should still be usable");
    assert_eq!(sequence, 9);
    assert_eq!(
        source,
        starcoin_node_mcp_types::SequenceNumberSource::Onchain
    );
}

fn sample_runtime_config() -> RuntimeConfig {
    sample_runtime_config_with_endpoint("https://example.com")
}

fn sample_runtime_config_with_endpoint(endpoint: &str) -> RuntimeConfig {
    let mut config =
        runtime_config_with_endpoint(endpoint, Mode::Transaction, VmProfile::Auto, true);
    config.connect_timeout = Duration::from_secs(3);
    config.request_timeout = Duration::from_secs(10);
    config.startup_probe_timeout = Duration::from_secs(10);
    config.chain_status_cache_ttl = Duration::from_secs(3);
    config.module_cache_max_entries = 1_024;
    config
}

fn sample_probe() -> EffectiveProbe {
    EffectiveProbe {
        supports_node_info: true,
        supports_chain_info: true,
        supports_block_lookup: true,
        supports_block_listing: true,
        supports_transaction_lookup: true,
        supports_transaction_info_lookup: true,
        supports_transaction_events_by_hash: true,
        supports_account_state_lookup: true,
        supports_events_query: true,
        supports_resource_listing: true,
        supports_module_listing: true,
        supports_abi_resolution: true,
        supports_view_call: true,
        supports_transaction_submission: true,
        supports_raw_dry_run: true,
    }
}

fn sample_chain_context(chain_id: u8, network: &str, genesis_hash: &str) -> ChainContext {
    ChainContext {
        chain_id,
        network: network.to_owned(),
        genesis_hash: genesis_hash.to_owned(),
        head_block_hash: "0x2".to_owned(),
        head_block_number: 42,
        head_state_root: Some("0x3".to_owned()),
        observed_at: "2026-03-25T00:00:00Z".to_owned(),
    }
}

fn sample_signed_transaction(chain_id: u8, sequence_number: u64) -> SignedUserTransaction {
    let sender = AccountAddress::from_str("0x1").expect("valid sender");
    let raw_txn = super::build_raw_transaction(
        sender,
        sequence_number,
        TransactionPayload::Script(Script::new(vec![], vec![], vec![])),
        10_000,
        1,
        1_000,
        "0x1::STC::STC".to_owned(),
        ChainId::new(chain_id),
    );
    let (private_key, public_key) = genesis_key_pair();
    raw_txn
        .sign(&private_key, public_key)
        .expect("sample transaction should sign")
        .into_inner()
}

fn sample_app_context() -> AppContext {
    let config = sample_runtime_config();
    AppContext {
        rpc: NodeRpcClient::new(&config).expect("rpc client should build"),
        watch_permits: Arc::new(Semaphore::new(config.max_concurrent_watch_requests)),
        expensive_permits: Arc::new(Semaphore::new(config.max_inflight_expensive_requests)),
        startup_probe: sample_probe(),
        transaction_probe: Arc::new(RwLock::new(Some(CachedProbe {
            probe: sample_probe(),
            observed_at: Instant::now(),
        }))),
        prepared_transactions: Arc::new(RwLock::new(HashMap::new())),
        unresolved_submissions: Arc::new(RwLock::new(HashMap::new())),
        config,
    }
}
