mod support;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use starmask_core::{CoordinatorCommand, CoordinatorConfig, CoordinatorResponse};
use starmask_types::{
    GetRequestStatusResult, LockState, PresentationId, RequestStatus, TimestampMs, WalletInstanceId,
};

use support::{
    BASE_TIME_MS, create_sign_transaction, mark_presented, open_coordinator,
    open_coordinator_with_config, pull_next_request, register_wallet, resolve_transaction_request,
    wallet_account,
};

#[test]
fn created_request_survives_restart_and_can_be_polled_by_id() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

    let request_id = {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        create_sign_transaction(&mut coordinator, "client-created", &wallet_instance_id).request_id
    };

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
    let status = restarted
        .dispatch(CoordinatorCommand::GetRequestStatus {
            request_id: request_id.clone(),
        })
        .unwrap();
    let CoordinatorResponse::RequestStatus(GetRequestStatusResult {
        status,
        request_id: got,
        ..
    }) = status
    else {
        panic!("unexpected response");
    };

    assert_eq!(got, request_id);
    assert_eq!(status, RequestStatus::Created);
}

#[test]
fn dispatched_request_requeues_after_restart_and_lease_expiry() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

    let request_id = {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-dispatched", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        assert_eq!(pulled.request_id, created.request_id);
        created.request_id
    };

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
    let status = restarted
        .dispatch(CoordinatorCommand::GetRequestStatus {
            request_id: request_id.clone(),
        })
        .unwrap();
    let CoordinatorResponse::RequestStatus(status) = status else {
        panic!("unexpected response");
    };
    assert_eq!(status.status, RequestStatus::Dispatched);

    let pulled = restarted
        .dispatch(CoordinatorCommand::PullNextRequest {
            wallet_instance_id: wallet_instance_id.clone(),
        })
        .unwrap();
    let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
        panic!("unexpected response");
    };
    assert!(pulled.request.is_none());

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 31_000),
    );
    restarted
        .dispatch(CoordinatorCommand::TickMaintenance)
        .unwrap();

    let pulled = pull_next_request(&mut restarted, &wallet_instance_id);
    assert_eq!(pulled.request_id, request_id);
    assert!(!pulled.resume_required);
    assert!(pulled.delivery_lease_id.is_some());
}

#[test]
fn pending_request_resumes_for_same_instance_after_restart() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
    let presentation_id = PresentationId::new("presentation-1").unwrap();

    let request_id = {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-pending", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            presentation_id.clone(),
        );
        created.request_id
    };

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
    let status = restarted
        .dispatch(CoordinatorCommand::GetRequestStatus {
            request_id: request_id.clone(),
        })
        .unwrap();
    let CoordinatorResponse::RequestStatus(status) = status else {
        panic!("unexpected response");
    };
    assert_eq!(status.status, RequestStatus::PendingUserApproval);

    let resumed = pull_next_request(&mut restarted, &wallet_instance_id);
    assert_eq!(resumed.request_id, request_id);
    assert!(resumed.resume_required);
    assert_eq!(resumed.presentation_id, Some(presentation_id));
    assert_eq!(resumed.delivery_lease_id, None);
}

#[test]
fn pending_request_is_not_redelivered_to_other_wallet_after_restart() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
    let other_wallet_instance_id = WalletInstanceId::new("wallet-2").unwrap();

    {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        register_wallet(
            &mut coordinator,
            &other_wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&other_wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-owned", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            PresentationId::new("presentation-owned").unwrap(),
        );
    }

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
    let pulled = restarted
        .dispatch(CoordinatorCommand::PullNextRequest {
            wallet_instance_id: other_wallet_instance_id,
        })
        .unwrap();
    let CoordinatorResponse::PullNextRequest(pulled) = pulled else {
        panic!("unexpected response");
    };
    assert!(pulled.request.is_none());
}

#[test]
fn approved_result_survives_restart_until_eviction_and_keeps_metadata() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
    let presentation_id = PresentationId::new("presentation-approved").unwrap();
    let request_id = {
        let mut config = CoordinatorConfig::default();
        config.result_retention = starmask_types::DurationSeconds::new(60);
        let mut coordinator = open_coordinator_with_config(
            &database_path,
            TimestampMs::from_millis(BASE_TIME_MS),
            config,
        );
        register_wallet(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-approved", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled
                .delivery_lease_id
                .expect("delivery lease should exist"),
            presentation_id.clone(),
        );
        resolve_transaction_request(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            presentation_id,
        );
        created.request_id
    };

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 10_000),
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

    let mut config = CoordinatorConfig::default();
    config.result_retention = starmask_types::DurationSeconds::new(60);
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
