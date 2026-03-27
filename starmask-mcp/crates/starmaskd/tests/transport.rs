mod support;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;
use tokio::task::JoinHandle;

use starmask_core::CoordinatorConfig;
use starmask_types::{Channel, JsonRpcResponse};
use starmaskd::{
    config::{
        LocalAccountDirBackendConfig, LocalPromptMode, StarmaskExtensionBackendConfig,
        WalletBackendConfig,
    },
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
            ServerPolicy::new(
                Channel::Development,
                vec![WalletBackendConfig::StarmaskExtension(
                    StarmaskExtensionBackendConfig::new(
                        "browser-default",
                        "Browser Default",
                        starmask_types::ApprovalSurface::BrowserUi,
                        ["ext.allowed".to_owned()].into_iter().collect(),
                        "com.starcoin.test",
                        None,
                    ),
                )],
            ),
        )
        .await
        .unwrap();
    });
    wait_for_socket(&socket_path).await;
    (tempdir, socket_path, server)
}

async fn spawn_local_backend_server() -> (tempfile::TempDir, std::path::PathBuf, JoinHandle<()>) {
    let tempdir = tempdir().unwrap();
    let socket_path = tempdir.path().join("starmaskd.sock");
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let store = SqliteStore::open(&database_path).unwrap();
    let handle = spawn_coordinator(store, CoordinatorConfig::default());
    let socket_path_for_server = socket_path.clone();
    let account_dir = tempdir.path().join("account");
    std::fs::create_dir_all(&account_dir).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&account_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let server = tokio::spawn(async move {
        run_unix_server(
            &socket_path_for_server,
            handle,
            ServerPolicy::new(
                Channel::Development,
                vec![WalletBackendConfig::LocalAccountDir(
                    LocalAccountDirBackendConfig::new(
                        "local-main",
                        "Local Main",
                        starmask_types::ApprovalSurface::TtyPrompt,
                        account_dir,
                        LocalPromptMode::TtyPrompt,
                        251,
                        starmask_types::DurationSeconds::new(30),
                        true,
                        false,
                    ),
                )],
            ),
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

fn local_backend_account(
    address: &str,
    public_key: &str,
    is_default: bool,
    is_locked: bool,
) -> serde_json::Value {
    json!({
        "address": address,
        "label": null,
        "public_key": public_key,
        "is_default": is_default,
        "is_read_only": false,
        "is_locked": is_locked,
    })
}

async fn register_local_backend(
    socket_path: &std::path::Path,
    lock_state: &str,
    accounts: Vec<serde_json::Value>,
) {
    let registered = call_daemon(
        socket_path,
        "req-register-local",
        "backend.register",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "backend_kind": "local_account_dir",
            "transport_kind": "local_socket",
            "approval_surface": "tty_prompt",
            "instance_label": "Local Main",
            "lock_state": lock_state,
            "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
            "backend_metadata": {
                "account_provider_kind": "local",
                "prompt_mode": "tty_prompt"
            },
            "accounts": accounts,
        }),
    )
    .await;
    let JsonRpcResponse::Success(registered) = registered else {
        panic!("expected register success");
    };
    assert_eq!(registered.result["accepted"], json!(true));
}

async fn list_wallet_instances(socket_path: &std::path::Path) -> serde_json::Value {
    let response = call_daemon(
        socket_path,
        "req-list-instances",
        "wallet.listInstances",
        json!({
            "protocol_version": 1,
            "connected_only": true,
        }),
    )
    .await;
    let JsonRpcResponse::Success(response) = response else {
        panic!("expected wallet.listInstances success");
    };
    response.result
}

async fn list_wallet_accounts(
    socket_path: &std::path::Path,
    wallet_instance_id: &str,
) -> serde_json::Value {
    let response = call_daemon(
        socket_path,
        "req-list-accounts",
        "wallet.listAccounts",
        json!({
            "protocol_version": 1,
            "wallet_instance_id": wallet_instance_id,
            "include_public_key": true,
        }),
    )
    .await;
    let JsonRpcResponse::Success(response) = response else {
        panic!("expected wallet.listAccounts success");
    };
    response.result
}

async fn has_available(socket_path: &std::path::Path, wallet_instance_id: &str) -> serde_json::Value {
    let response = call_daemon(
        socket_path,
        "req-has-available",
        "request.hasAvailable",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": wallet_instance_id,
        }),
    )
    .await;
    let JsonRpcResponse::Success(response) = response else {
        panic!("expected request.hasAvailable success");
    };
    response.result
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
async fn unix_server_round_trips_generic_backend_register_and_resolve() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    register_local_backend(
        &socket_path,
        "unlocked",
        vec![local_backend_account("0x1", "0xabc", true, false)],
    )
    .await;

    let created = call_daemon(
        &socket_path,
        "req-create-local",
        "request.createSignMessage",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-local-sign",
            "account_address": "0x1",
            "wallet_instance_id": "local-main",
            "message": "hello",
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    let request_id = created.result["request_id"].as_str().unwrap();

    let pulled = call_daemon(
        &socket_path,
        "req-pull-local",
        "request.pullNext",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
        }),
    )
    .await;
    let JsonRpcResponse::Success(pulled) = pulled else {
        panic!("expected pull success");
    };
    let request = pulled.result["request"].clone();
    assert_eq!(request["request_id"], json!(request_id));
    let delivery_lease_id = request["delivery_lease_id"].as_str().unwrap();

    let presented = call_daemon(
        &socket_path,
        "req-presented-local",
        "request.presented",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "request_id": request_id,
            "delivery_lease_id": delivery_lease_id,
            "presentation_id": "presentation-1",
        }),
    )
    .await;
    let JsonRpcResponse::Success(presented) = presented else {
        panic!("expected presented success");
    };
    assert_eq!(presented.result["ok"], json!(true));

    let resolved = call_daemon(
        &socket_path,
        "req-resolve-local",
        "request.resolve",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "request_id": request_id,
            "presentation_id": "presentation-1",
            "result_kind": "signed_message",
            "signature": "0xsigned-message",
        }),
    )
    .await;
    let JsonRpcResponse::Success(resolved) = resolved else {
        panic!("expected resolve success");
    };
    assert_eq!(resolved.result["ok"], json!(true));

    let status = call_daemon(
        &socket_path,
        "req-status-local",
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
    assert_eq!(status.result["status"], json!("approved"));
    assert_eq!(
        status.result["result"],
        json!({
            "kind": "signed_message",
            "signature": "0xsigned-message"
        })
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_round_trips_generic_backend_reject() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    register_local_backend(
        &socket_path,
        "unlocked",
        vec![local_backend_account("0x1", "0xabc", true, false)],
    )
    .await;

    let created = call_daemon(
        &socket_path,
        "req-create-local-reject",
        "request.createSignMessage",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-local-reject",
            "account_address": "0x1",
            "wallet_instance_id": "local-main",
            "message": "hello",
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    let request_id = created.result["request_id"].as_str().unwrap();

    let pulled = call_daemon(
        &socket_path,
        "req-pull-local-reject",
        "request.pullNext",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
        }),
    )
    .await;
    let JsonRpcResponse::Success(pulled) = pulled else {
        panic!("expected pull success");
    };
    let delivery_lease_id = pulled.result["request"]["delivery_lease_id"].as_str().unwrap();

    let presented = call_daemon(
        &socket_path,
        "req-presented-local-reject",
        "request.presented",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "request_id": request_id,
            "delivery_lease_id": delivery_lease_id,
            "presentation_id": "presentation-reject",
        }),
    )
    .await;
    let JsonRpcResponse::Success(presented) = presented else {
        panic!("expected presented success");
    };
    assert_eq!(presented.result["ok"], json!(true));

    let rejected = call_daemon(
        &socket_path,
        "req-reject-local",
        "request.reject",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "request_id": request_id,
            "presentation_id": "presentation-reject",
            "reason_code": "request_rejected",
            "reason_message": "User rejected the signing request",
        }),
    )
    .await;
    let JsonRpcResponse::Success(rejected) = rejected else {
        panic!("expected reject success");
    };
    assert_eq!(rejected.result["ok"], json!(true));

    let status = call_daemon(
        &socket_path,
        "req-status-local-reject",
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
    assert_eq!(status.result["status"], json!("rejected"));
    assert_eq!(status.result["error_code"], json!("request_rejected"));
    assert_eq!(
        status.result["error_message"],
        json!("User rejected the signing request")
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_reports_request_has_available_for_local_backend() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    register_local_backend(
        &socket_path,
        "unlocked",
        vec![local_backend_account("0x1", "0xabc", true, false)],
    )
    .await;

    let available = has_available(&socket_path, "local-main").await;
    assert_eq!(available["available"], json!(false));

    let created = call_daemon(
        &socket_path,
        "req-create-available",
        "request.createSignMessage",
        json!({
            "protocol_version": 1,
            "client_request_id": "client-has-available",
            "account_address": "0x1",
            "wallet_instance_id": "local-main",
            "message": "hello",
            "format": "utf8",
        }),
    )
    .await;
    let JsonRpcResponse::Success(created) = created else {
        panic!("expected create success");
    };
    assert_eq!(created.result["status"], json!("created"));

    let available = has_available(&socket_path, "local-main").await;
    assert_eq!(available["available"], json!(true));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_repeated_empty_pull_next_remains_stable_for_local_backend() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    register_local_backend(
        &socket_path,
        "unlocked",
        vec![local_backend_account("0x1", "0xabc", true, false)],
    )
    .await;

    let start = Instant::now();
    for attempt in 0..32 {
        let pulled = call_daemon(
            &socket_path,
            &format!("req-pull-empty-{attempt}"),
            "request.pullNext",
            json!({
                "protocol_version": 2,
                "wallet_instance_id": "local-main",
            }),
        )
        .await;
        let JsonRpcResponse::Success(pulled) = pulled else {
            panic!("expected pull success");
        };
        assert!(pulled.result["request"].is_null());
    }
    assert!(start.elapsed() < Duration::from_secs(2));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_rejects_unknown_local_backend_registration() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    let response = call_daemon(
        &socket_path,
        "req-register-unknown-local",
        "backend.register",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-unknown",
            "backend_kind": "local_account_dir",
            "transport_kind": "local_socket",
            "approval_surface": "tty_prompt",
            "instance_label": "Local Unknown",
            "lock_state": "unlocked",
            "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
            "backend_metadata": {
                "account_provider_kind": "local",
                "prompt_mode": "tty_prompt"
            },
            "accounts": [],
        }),
    )
    .await;
    let JsonRpcResponse::Error(response) = response else {
        panic!("expected backend registration failure");
    };
    assert_eq!(
        response.error.code,
        starmask_types::SharedErrorCode::BackendNotAllowed
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_rejects_generic_backend_registration_over_v1_protocol() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    let response = call_daemon(
        &socket_path,
        "req-register-local-v1",
        "backend.register",
        json!({
            "protocol_version": 1,
            "wallet_instance_id": "local-main",
            "backend_kind": "local_account_dir",
            "transport_kind": "local_socket",
            "approval_surface": "tty_prompt",
            "instance_label": "Local Main",
            "lock_state": "unlocked",
            "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
            "backend_metadata": {
                "account_provider_kind": "local",
                "prompt_mode": "tty_prompt"
            },
            "accounts": [],
        }),
    )
    .await;
    let JsonRpcResponse::Error(response) = response else {
        panic!("expected backend registration failure");
    };
    assert_eq!(
        response.error.code,
        starmask_types::SharedErrorCode::ProtocolVersionMismatch
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_rejects_local_backend_registration_when_backend_is_not_enabled() {
    let (_tempdir, socket_path, server) = spawn_test_server().await;

    let response = call_daemon(
        &socket_path,
        "req-register-disabled-local",
        "backend.register",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "backend_kind": "local_account_dir",
            "transport_kind": "local_socket",
            "approval_surface": "tty_prompt",
            "instance_label": "Local Main",
            "lock_state": "unlocked",
            "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
            "backend_metadata": {
                "account_provider_kind": "local",
                "prompt_mode": "tty_prompt"
            },
            "accounts": [],
        }),
    )
    .await;
    let JsonRpcResponse::Error(response) = response else {
        panic!("expected backend registration failure");
    };
    assert_eq!(
        response.error.code,
        starmask_types::SharedErrorCode::BackendNotAllowed
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_backend_heartbeat_updates_lock_state() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    register_local_backend(
        &socket_path,
        "unlocked",
        vec![local_backend_account("0x1", "0xabc", true, false)],
    )
    .await;

    let heartbeat = call_daemon(
        &socket_path,
        "req-heartbeat-local",
        "backend.heartbeat",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "presented_request_ids": [],
            "lock_state": "locked",
        }),
    )
    .await;
    let JsonRpcResponse::Success(heartbeat) = heartbeat else {
        panic!("expected heartbeat success");
    };
    assert_eq!(heartbeat.result["ok"], json!(true));

    let instances = list_wallet_instances(&socket_path).await;
    assert_eq!(
        instances["wallet_instances"][0]["wallet_instance_id"],
        json!("local-main")
    );
    assert_eq!(
        instances["wallet_instances"][0]["lock_state"],
        json!("locked")
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn unix_server_backend_update_accounts_replaces_snapshot() {
    let (_tempdir, socket_path, server) = spawn_local_backend_server().await;

    register_local_backend(
        &socket_path,
        "unlocked",
        vec![local_backend_account("0x1", "0xabc", true, false)],
    )
    .await;

    let updated = call_daemon(
        &socket_path,
        "req-update-accounts-local",
        "backend.updateAccounts",
        json!({
            "protocol_version": 2,
            "wallet_instance_id": "local-main",
            "lock_state": "locked",
            "capabilities": ["unlock", "get_public_key", "sign_message", "sign_transaction"],
            "accounts": [
                local_backend_account("0x2", "0xdef", true, false)
            ],
        }),
    )
    .await;
    let JsonRpcResponse::Success(updated) = updated else {
        panic!("expected update accounts success");
    };
    assert_eq!(updated.result["ok"], json!(true));

    let accounts = list_wallet_accounts(&socket_path, "local-main").await;
    assert_eq!(
        accounts["wallet_instances"][0]["wallet_instance_id"],
        json!("local-main")
    );
    assert_eq!(
        accounts["wallet_instances"][0]["lock_state"],
        json!("locked")
    );
    assert_eq!(
        accounts["wallet_instances"][0]["accounts"],
        json!([{
            "address": "0x2",
            "public_key": "0xdef",
            "is_default": true,
            "is_locked": false
        }])
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

    let socket_mode = std::fs::metadata(&socket_path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
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
