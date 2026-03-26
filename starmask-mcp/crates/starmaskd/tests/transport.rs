mod support;

use std::collections::BTreeSet;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;
use tokio::task::JoinHandle;

use starmask_core::CoordinatorConfig;
use starmask_types::{Channel, JsonRpcResponse};
use starmaskd::{
    coordinator_runtime::spawn_coordinator,
    server::{ServerPolicy, run_unix_server},
    sqlite_store::SqliteStore,
};

use support::{call_daemon, native_bridge_account, wait_for_socket};

async fn spawn_test_server() -> (tempfile::TempDir, std::path::PathBuf, JoinHandle<()>) {
    let tempdir = tempdir().unwrap();
    let socket_path = tempdir.path().join("starmaskd.sock");
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let store = SqliteStore::open(&database_path).unwrap();
    let handle = spawn_coordinator(store, CoordinatorConfig::default());
    let socket_path_for_server = socket_path.clone();
    let server = tokio::spawn(async move {
        run_unix_server(
            &socket_path_for_server,
            handle,
            ServerPolicy {
                channel: Channel::Development,
                allowed_extension_ids: BTreeSet::from(["ext.allowed".to_owned()]),
                native_host_name: "com.starcoin.test".to_owned(),
            },
        )
        .await
        .unwrap();
    });
    wait_for_socket(&socket_path).await;
    (tempdir, socket_path, server)
}

async fn register_wallet(socket_path: &std::path::Path, wallet_instance_id: &str) {
    let response = call_daemon(
        socket_path,
        "req-register",
        "extension.register",
        json!({
            "protocol_version": 1,
            "wallet_instance_id": wallet_instance_id,
            "extension_id": "ext.allowed",
            "extension_version": "1.0.0",
            "profile_hint": "default",
            "lock_state": "unlocked",
            "accounts_summary": [native_bridge_account("0x1", true)],
        }),
    )
    .await;
    let JsonRpcResponse::Success(response) = response else {
        panic!("expected register success");
    };
    assert_eq!(response.result["accepted"], json!(true));
}

#[tokio::test]
async fn unix_server_round_trips_extension_register_create_and_status() {
    let (_tempdir, socket_path, server) = spawn_test_server().await;

    register_wallet(&socket_path, "wallet-1").await;

    let created = call_daemon(
        &socket_path,
        "req-create",
        "request.createSignTransaction",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-transport",
            "account_address": "0x1",
            "wallet_instance_id": "wallet-1",
            "chain_id": 251,
            "raw_txn_bcs_hex": "0xabc",
            "tx_kind": "transfer",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    assert_eq!(created.id, "req-create");
    assert_eq!(created.result["status"], json!("created"));
    let request_id = created.result["request_id"]
        .as_str()
        .expect("request id should be present");

    let status = call_daemon(
        &socket_path,
        "req-status",
        "request.getStatus",
        json!({
            "protocol_version": 1,
            "request_id": request_id,
        }),
    )
    .await;
    let JsonRpcResponse::Success(status) = status else {
        panic!("expected status success");
    };
    assert_eq!(status.result["status"], json!("created"));
    assert_eq!(status.result["request_id"], json!(request_id));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_preserves_request_id_for_idempotent_retry() {
    let (_tempdir, socket_path, server) = spawn_test_server().await;
    register_wallet(&socket_path, "wallet-1").await;

    let first = call_daemon(
        &socket_path,
        "req-create-1",
        "request.createSignTransaction",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-idempotent",
            "account_address": "0x1",
            "wallet_instance_id": "wallet-1",
            "chain_id": 251,
            "raw_txn_bcs_hex": "0xabc",
            "tx_kind": "transfer",
        }),
    )
    .await;
    let second = call_daemon(
        &socket_path,
        "req-create-2",
        "request.createSignTransaction",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-idempotent",
            "account_address": "0x1",
            "wallet_instance_id": "wallet-1",
            "chain_id": 251,
            "raw_txn_bcs_hex": "0xabc",
            "tx_kind": "transfer",
        }),
    )
    .await;

    let JsonRpcResponse::Success(first) = first else {
        panic!("expected first create success");
    };
    let JsonRpcResponse::Success(second) = second else {
        panic!("expected second create success");
    };
    assert_eq!(first.result["request_id"], second.result["request_id"]);

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_reports_idempotency_conflict_for_changed_payload() {
    let (_tempdir, socket_path, server) = spawn_test_server().await;
    register_wallet(&socket_path, "wallet-1").await;

    let _first = call_daemon(
        &socket_path,
        "req-create-1",
        "request.createSignTransaction",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-conflict",
            "account_address": "0x1",
            "wallet_instance_id": "wallet-1",
            "chain_id": 251,
            "raw_txn_bcs_hex": "0xabc",
            "tx_kind": "transfer",
        }),
    )
    .await;
    let second = call_daemon(
        &socket_path,
        "req-create-2",
        "request.createSignTransaction",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-conflict",
            "account_address": "0x1",
            "wallet_instance_id": "wallet-1",
            "chain_id": 251,
            "raw_txn_bcs_hex": "0xdef",
            "tx_kind": "transfer",
        }),
    )
    .await;

    let JsonRpcResponse::Error(second) = second else {
        panic!("expected idempotency conflict");
    };
    assert_eq!(
        second.error.code,
        starmask_types::SharedErrorCode::IdempotencyKeyConflict
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_rejects_disallowed_extension_over_transport() {
    let (_tempdir, socket_path, server) = spawn_test_server().await;

    let response = call_daemon(
        &socket_path,
        "req-blocked",
        "extension.register",
        json!({
            "protocol_version": 1,
            "wallet_instance_id": "wallet-1",
            "extension_id": "ext.blocked",
            "extension_version": "1.0.0",
            "profile_hint": "default",
            "lock_state": "unlocked",
            "accounts_summary": [],
        }),
    )
    .await;
    let JsonRpcResponse::Success(response) = response else {
        panic!("expected register response");
    };
    assert_eq!(response.id, "req-blocked");
    assert_eq!(response.result["accepted"], json!(false));

    server.abort();
    let _ = server.await;
}

#[cfg(unix)]
#[tokio::test]
async fn unix_server_locks_down_socket_permissions() {
    let (_tempdir, socket_path, server) = spawn_test_server().await;

    let socket_mode = std::fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777;
    let parent_mode = std::fs::metadata(socket_path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(socket_mode, 0o600);
    assert_eq!(parent_mode, 0o700);

    server.abort();
    let _ = server.await;
}
