mod support;

use pretty_assertions::assert_eq;
use rusqlite::{Connection, params};
use tempfile::tempdir;

use starmask_core::{CoordinatorCommand, CoordinatorConfig, CoordinatorResponse};
use starmask_types::{PresentationId, RequestStatus, TimestampMs, WalletInstanceId};

use support::{
    BASE_TIME_MS, create_sign_transaction, mark_presented, open_coordinator_with_config,
    pull_next_request, resolve_transaction_request,
};

#[test]
fn migrated_extension_wallets_still_route_requests_and_evict_results() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .execute(
            "INSERT INTO wallet_instances (
                wallet_instance_id, extension_id, extension_version, protocol_version,
                profile_hint, lock_state, connected, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                wallet_instance_id.as_str(),
                "ext.allowed",
                "1.2.3",
                1,
                "Browser Default",
                "unlocked",
                1,
                BASE_TIME_MS
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO wallet_accounts (
                wallet_instance_id, address, label, public_key, is_default, is_locked, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                wallet_instance_id.as_str(),
                "0x1",
                Option::<String>::None,
                "0xabc",
                1,
                0,
                BASE_TIME_MS
            ],
        )
        .unwrap();
    drop(connection);

    let mut config = CoordinatorConfig::default();
    config.result_retention = starmask_types::DurationSeconds::new(60);
    let request_id = {
        let mut coordinator = open_coordinator_with_config(
            &database_path,
            TimestampMs::from_millis(BASE_TIME_MS),
            config.clone(),
        );

        let listed = coordinator
            .dispatch(CoordinatorCommand::WalletListAccounts {
                wallet_instance_id: Some(wallet_instance_id.clone()),
                include_public_key: true,
            })
            .unwrap();
        let CoordinatorResponse::WalletAccounts(listed) = listed else {
            panic!("unexpected response");
        };
        assert_eq!(listed.wallet_instances.len(), 1);
        assert_eq!(
            listed.wallet_instances[0].wallet_instance_id,
            wallet_instance_id
        );
        assert_eq!(listed.wallet_instances[0].accounts.len(), 1);
        assert_eq!(listed.wallet_instances[0].accounts[0].address, "0x1");
        assert_eq!(
            listed.wallet_instances[0].accounts[0].public_key.as_deref(),
            Some("0xabc")
        );

        let created = create_sign_transaction(
            &mut coordinator,
            "client-migrated-extension",
            &wallet_instance_id,
        );
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            PresentationId::new("presentation-migrated-extension").unwrap(),
        );
        resolve_transaction_request(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            PresentationId::new("presentation-migrated-extension").unwrap(),
        );
        created.request_id
    };

    let mut restarted = open_coordinator_with_config(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 10_000),
        config.clone(),
    );
    let status = restarted
        .dispatch(CoordinatorCommand::GetRequestStatus {
            request_id: request_id.clone(),
        })
        .unwrap();
    let CoordinatorResponse::RequestStatus(status) = status else {
        panic!("unexpected response");
    };
    assert_eq!(status.status, RequestStatus::Approved);
    assert!(status.result.is_some());
    assert!(status.result_available);

    let mut restarted = open_coordinator_with_config(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 61_000),
        config,
    );
    restarted
        .dispatch(CoordinatorCommand::TickMaintenance)
        .unwrap();
    let status = restarted
        .dispatch(CoordinatorCommand::GetRequestStatus { request_id })
        .unwrap();
    let CoordinatorResponse::RequestStatus(status) = status else {
        panic!("unexpected response");
    };
    assert_eq!(status.status, RequestStatus::Approved);
    assert_eq!(status.result, None);
    assert!(!status.result_available);
    assert_eq!(status.result_expires_at, None);
}
