use super::{
    AppContext, CachedProbe, SignedUserTransaction, accepted_submission_output,
    effective_submit_timeout_seconds, enforce_transaction_head_lag, extract_chain_context,
    is_terminal_watch_status, status_summary_from_parts, submission_unknown_output,
    validate_chain_identity, validate_signed_transaction_submission, validate_transaction_probe,
};
use httpmock::{Mock, prelude::*};
use serde_json::{Value, json};
use starcoin_node_mcp_rpc::NodeRpcClient;
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
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, Semaphore};
use url::Url;

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
    mock_json_rpc_result(&server, "node.status", json!(true));
    mock_json_rpc_result(&server, "chain.info", sample_chain_info_value());
    mock_json_rpc_result(&server, "node.info", sample_node_info_value());
    mock_json_rpc_result(&server, "chain.get_block_by_number", Value::Null);
    mock_json_rpc_result(&server, "chain.get_blocks_by_number", json!([]));
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

fn sample_runtime_config() -> RuntimeConfig {
    sample_runtime_config_with_endpoint("https://example.com")
}

fn sample_runtime_config_with_endpoint(endpoint: &str) -> RuntimeConfig {
    RuntimeConfig {
        rpc_endpoint_url: Url::parse(endpoint).expect("valid url"),
        mode: Mode::Transaction,
        vm_profile: VmProfile::Auto,
        expected_chain_id: Some(254),
        expected_network: Some("main".to_owned()),
        expected_genesis_hash: Some("0x1".to_owned()),
        require_genesis_hash_match: true,
        connect_timeout: Duration::from_secs(3),
        request_timeout: Duration::from_secs(10),
        startup_probe_timeout: Duration::from_secs(10),
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
        chain_status_cache_ttl: Duration::from_secs(3),
        abi_cache_ttl: Duration::from_secs(300),
        module_cache_max_entries: 1_024,
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

fn sample_node_info_value() -> Value {
    json!({
        "net": { "Builtin": "Main" },
        "now_seconds": 120,
    })
}

fn sample_chain_info_value() -> Value {
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
