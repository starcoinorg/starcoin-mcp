mod support;

use pretty_assertions::assert_eq;
use serde_json::json;
use starmask_mcp::{AdapterError, StarmaskMcpServer};
use starmask_types::{
    CancelRequestResult, CreateSignTransactionParams, DurationSeconds, RequestId, RequestKind,
    RequestStatus,
};

use self::support::{
    FakeDaemonClient, FakeDaemonResponses, sample_create_request_result,
    sample_message_status_result, sample_sign_message_params, sample_wallet_public_key_result,
    sample_wallet_status_result, wallet_instance_id,
};

#[tokio::test]
async fn call_tool_json_wallet_status_serializes_response() {
    let wallet_instance_id = wallet_instance_id();
    let expected = sample_wallet_status_result(&wallet_instance_id);
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        wallet_status: Some(expected.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon);

    let result = server
        .call_tool_json("wallet_status", None)
        .await
        .expect("tool call should succeed");

    assert_eq!(
        result,
        serde_json::to_value(expected).expect("result should serialize")
    );
}

#[tokio::test]
async fn call_tool_json_get_public_key_tracks_target_wallet_instance() {
    let wallet_instance_id = wallet_instance_id();
    let expected = sample_wallet_public_key_result(&wallet_instance_id);
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        wallet_get_public_key: Some(expected.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json(
            "wallet_get_public_key",
            Some(
                json!({
                    "address": "0x1",
                    "wallet_instance_id": wallet_instance_id.as_str(),
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_get_public_key,
        Some(("0x1".to_owned(), Some(wallet_instance_id)))
    );
    assert_eq!(
        result,
        serde_json::to_value(expected).expect("result should serialize")
    );
}

#[tokio::test]
async fn call_tool_json_sign_transaction_maps_host_request_to_daemon_params() {
    let wallet_instance_id = wallet_instance_id();
    let expected_result = sample_create_request_result(RequestKind::SignTransaction);
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        create_sign_transaction_request: Some(expected_result.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json(
            "wallet_request_sign_transaction",
            Some(
                json!({
                    "client_request_id": "client-1",
                    "account_address": "0x1",
                    "wallet_instance_id": wallet_instance_id.as_str(),
                    "chain_id": 254,
                    "raw_txn_bcs_hex": "0xdeadbeef",
                    "tx_kind": "transfer",
                    "display_hint": "Send STC",
                    "client_context": "host-context",
                    "ttl_seconds": 90,
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_sign_transaction_request,
        Some(CreateSignTransactionParams {
            protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
            client_request_id: starmask_types::ClientRequestId::new("client-1")
                .expect("client request id should be valid"),
            account_address: "0x1".to_owned(),
            wallet_instance_id: Some(wallet_instance_id),
            chain_id: 254,
            raw_txn_bcs_hex: "0xdeadbeef".to_owned(),
            tx_kind: "transfer".to_owned(),
            display_hint: Some("Send STC".to_owned()),
            client_context: Some("host-context".to_owned()),
            ttl_seconds: Some(DurationSeconds::new(90)),
        })
    );
    assert_eq!(
        result,
        serde_json::to_value(expected_result).expect("result should serialize")
    );
}

#[tokio::test]
async fn call_tool_json_sign_message_maps_format_and_ttl() {
    let expected_request = sample_sign_message_params();
    let expected_result = sample_create_request_result(RequestKind::SignMessage);
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        create_sign_message_request: Some(expected_result.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json(
            "wallet_sign_message",
            Some(
                json!({
                    "client_request_id": "client-1",
                    "account_address": "0x1",
                    "wallet_instance_id": wallet_instance_id().as_str(),
                    "message": "68656c6c6f",
                    "format": "hex",
                    "display_hint": "Sign hello",
                    "client_context": "context",
                    "ttl_seconds": 90,
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_sign_message_request,
        Some(expected_request)
    );
    assert_eq!(
        result,
        serde_json::to_value(expected_result).expect("result should serialize")
    );
}

#[tokio::test]
async fn call_tool_json_get_request_status_parses_string_ids() {
    let wallet_instance_id = wallet_instance_id();
    let expected = sample_message_status_result(&wallet_instance_id);
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        get_request_status: Some(expected.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json(
            "wallet_get_request_status",
            Some(
                json!({
                    "request_id": "request-1",
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_get_request_status,
        Some(RequestId::new("request-1").expect("request id should be valid"))
    );
    assert_eq!(
        result,
        serde_json::to_value(expected).expect("result should serialize")
    );
}

#[tokio::test]
async fn call_tool_json_cancel_request_parses_string_ids() {
    let expected = CancelRequestResult {
        request_id: RequestId::new("request-1").expect("request id should be valid"),
        status: RequestStatus::Cancelled,
        error_code: None,
    };
    let daemon = FakeDaemonClient::with_responses(FakeDaemonResponses {
        cancel_request: Some(expected.clone()),
        ..Default::default()
    });
    let server = StarmaskMcpServer::new(daemon.clone());

    let result = server
        .call_tool_json(
            "wallet_cancel_request",
            Some(
                json!({
                    "request_id": "request-1",
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect("tool call should succeed");

    assert_eq!(
        daemon.state().last_cancel_request,
        Some(RequestId::new("request-1").expect("request id should be valid"))
    );
    assert_eq!(
        result,
        serde_json::to_value(expected).expect("result should serialize")
    );
}

#[tokio::test]
async fn invalid_wallet_instance_id_is_reported_as_invalid_request() {
    let server = StarmaskMcpServer::new(FakeDaemonClient::with_responses(
        FakeDaemonResponses::default(),
    ));

    let error = server
        .call_tool_json(
            "wallet_get_public_key",
            Some(
                json!({
                    "address": "0x1",
                    "wallet_instance_id": "   ",
                })
                .as_object()
                .expect("tool arguments should be an object")
                .clone(),
            ),
        )
        .await
        .expect_err("invalid wallet instance id should fail");

    match error {
        AdapterError::InvalidRequest(message) => {
            assert!(message.contains("WalletInstanceId"));
            assert!(message.contains("cannot be empty"));
        }
        other => panic!("expected invalid request error, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_tool_is_reported_as_invalid_request() {
    let server = StarmaskMcpServer::new(FakeDaemonClient::with_responses(
        FakeDaemonResponses::default(),
    ));

    let error = server
        .call_tool_json("wallet_not_real", None)
        .await
        .expect_err("unknown tool should fail");

    match error {
        AdapterError::InvalidRequest(message) => {
            assert!(message.contains("unknown tool"));
            assert!(message.contains("wallet_not_real"));
        }
        other => panic!("expected invalid request error, got {other:?}"),
    }
}
