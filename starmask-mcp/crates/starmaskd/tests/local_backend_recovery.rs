mod support;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use starmask_core::{CoordinatorCommand, CoordinatorResponse};
use starmask_types::{
    GetRequestStatusResult, LockState, PresentationId, RequestStatus, TimestampMs,
    WalletInstanceId,
};

use support::{
    BASE_TIME_MS, create_sign_transaction, mark_presented, open_coordinator, pull_next_request,
    register_local_backend, wallet_account,
};

#[test]
fn local_backend_registration_record_survives_restart() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("local-main").unwrap();

    {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_local_backend(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
    }

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
    let listed = restarted
        .dispatch(CoordinatorCommand::WalletListInstances {
            connected_only: false,
        })
        .unwrap();
    let CoordinatorResponse::WalletInstances(listed) = listed else {
        panic!("unexpected response");
    };

    assert_eq!(listed.wallet_instances.len(), 1);
    assert_eq!(listed.wallet_instances[0].wallet_instance_id, wallet_instance_id);
    assert_eq!(listed.wallet_instances[0].lock_state, LockState::Unlocked);
}

#[test]
fn local_backend_created_request_survives_restart_and_can_be_polled_by_id() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("local-main").unwrap();

    let request_id = {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_local_backend(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        create_sign_transaction(&mut coordinator, "client-created-local", &wallet_instance_id)
            .request_id
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
fn local_backend_dispatched_request_requeues_after_restart_and_lease_expiry() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("local-main").unwrap();

    let request_id = {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_local_backend(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-dispatched-local", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        assert_eq!(pulled.request_id, created.request_id);
        created.request_id
    };

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
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

    let requeued = pull_next_request(&mut restarted, &wallet_instance_id);
    assert_eq!(requeued.request_id, request_id);
    assert!(!requeued.resume_required);
    assert!(requeued.delivery_lease_id.is_some());
}

#[test]
fn local_backend_pending_request_resumes_for_same_instance_after_restart() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("local-main").unwrap();
    let presentation_id = PresentationId::new("presentation-local").unwrap();

    let request_id = {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_local_backend(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-pending-local", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled.delivery_lease_id.expect("delivery lease should exist"),
            presentation_id.clone(),
        );
        created.request_id
    };

    let mut restarted = open_coordinator(
        &database_path,
        TimestampMs::from_millis(BASE_TIME_MS + 1_000),
    );
    let resumed = pull_next_request(&mut restarted, &wallet_instance_id);
    assert_eq!(resumed.request_id, request_id);
    assert!(resumed.resume_required);
    assert_eq!(resumed.presentation_id, Some(presentation_id));
    assert_eq!(resumed.delivery_lease_id, None);
}

#[test]
fn local_backend_pending_request_is_not_redelivered_to_other_backend_after_restart() {
    let tempdir = tempdir().unwrap();
    let database_path = tempdir.path().join("starmaskd.sqlite3");
    let wallet_instance_id = WalletInstanceId::new("local-main").unwrap();
    let other_wallet_instance_id = WalletInstanceId::new("local-other").unwrap();

    {
        let mut coordinator =
            open_coordinator(&database_path, TimestampMs::from_millis(BASE_TIME_MS));
        register_local_backend(
            &mut coordinator,
            &wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&wallet_instance_id, "0x1", true)],
        );
        register_local_backend(
            &mut coordinator,
            &other_wallet_instance_id,
            LockState::Unlocked,
            vec![wallet_account(&other_wallet_instance_id, "0x1", true)],
        );
        let created =
            create_sign_transaction(&mut coordinator, "client-owned-local", &wallet_instance_id);
        let pulled = pull_next_request(&mut coordinator, &wallet_instance_id);
        mark_presented(
            &mut coordinator,
            &created.request_id,
            &wallet_instance_id,
            pulled.delivery_lease_id.expect("delivery lease should exist"),
            PresentationId::new("presentation-owned-local").unwrap(),
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
