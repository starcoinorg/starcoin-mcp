use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use tracing::warn;

use starmask_types::{
    GetRequestStatusParams, NativeBridgeRequest, NativeBridgeResponse, RequestHasAvailableParams,
    RequestStatus, WalletInstanceId,
};

use crate::{
    client::{DaemonRpc, daemon_protocol_version},
    framing::write_frame,
};

const NOTIFICATION_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct NotificationState {
    wallets: BTreeMap<String, WalletWatchState>,
    active_requests: BTreeMap<String, WalletInstanceId>,
}

#[derive(Clone, Debug)]
struct WalletWatchState {
    wallet_instance_id: WalletInstanceId,
    last_available_notified: bool,
}

impl NotificationState {
    pub fn observe(&mut self, request: &NativeBridgeRequest, response: &NativeBridgeResponse) {
        match request {
            NativeBridgeRequest::ExtensionRegister {
                wallet_instance_id, ..
            } => {
                if matches!(
                    response,
                    NativeBridgeResponse::ExtensionRegistered { accepted: true, .. }
                ) {
                    self.ensure_wallet(wallet_instance_id.clone());
                }
            }
            NativeBridgeRequest::ExtensionHeartbeat {
                wallet_instance_id, ..
            }
            | NativeBridgeRequest::ExtensionUpdateAccounts {
                wallet_instance_id, ..
            } => {
                self.ensure_wallet(wallet_instance_id.clone());
            }
            NativeBridgeRequest::RequestPullNext {
                wallet_instance_id, ..
            } => {
                self.ensure_wallet(wallet_instance_id.clone());
                if let Some(wallet) = self.wallets.get_mut(wallet_instance_id.as_str()) {
                    wallet.last_available_notified = false;
                }
                if let NativeBridgeResponse::RequestNext {
                    request_id,
                    resume_required: true,
                    ..
                } = response
                {
                    self.active_requests
                        .insert(request_id.to_string(), wallet_instance_id.clone());
                }
            }
            NativeBridgeRequest::RequestPresented {
                request_id,
                wallet_instance_id,
                ..
            } => {
                if matches!(response, NativeBridgeResponse::ExtensionAck { .. }) {
                    self.active_requests
                        .insert(request_id.to_string(), wallet_instance_id.clone());
                }
            }
            NativeBridgeRequest::RequestResolve { request_id, .. }
            | NativeBridgeRequest::RequestReject { request_id, .. } => {
                if matches!(response, NativeBridgeResponse::ExtensionAck { .. }) {
                    self.active_requests.remove(request_id.as_str());
                }
            }
        }
    }

    fn ensure_wallet(&mut self, wallet_instance_id: WalletInstanceId) {
        self.wallets
            .entry(wallet_instance_id.to_string())
            .or_insert_with(|| WalletWatchState {
                wallet_instance_id,
                last_available_notified: false,
            });
    }
}

pub fn spawn_notification_loop<D>(
    client: D,
    state: Arc<Mutex<NotificationState>>,
    writer: Arc<Mutex<io::Stdout>>,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()>
where
    D: DaemonRpc + Clone + Send + 'static,
{
    thread::spawn(move || {
        while running.load(Ordering::Relaxed) {
            poll_available(&client, &state, &writer);
            poll_active_requests(&client, &state, &writer);
            thread::sleep(NOTIFICATION_POLL_INTERVAL);
        }
    })
}

fn poll_available<D>(
    client: &D,
    state: &Arc<Mutex<NotificationState>>,
    writer: &Arc<Mutex<io::Stdout>>,
) where
    D: DaemonRpc,
{
    let wallets = {
        let state = state.lock().expect("notification state poisoned");
        state.wallets.values().cloned().collect::<Vec<_>>()
    };

    for wallet in wallets {
        match client.request_has_available(RequestHasAvailableParams {
            protocol_version: daemon_protocol_version(),
            wallet_instance_id: wallet.wallet_instance_id.clone(),
        }) {
            Ok(result) => {
                let mut state = state.lock().expect("notification state poisoned");
                let Some(entry) = state.wallets.get_mut(result.wallet_instance_id.as_str()) else {
                    continue;
                };
                if result.available && !entry.last_available_notified {
                    if let Err(error) = emit_notification(
                        writer,
                        &NativeBridgeResponse::RequestAvailable {
                            wallet_instance_id: result.wallet_instance_id.clone(),
                        },
                    ) {
                        warn!(%error, "failed to emit request.available notification");
                    } else {
                        entry.last_available_notified = true;
                    }
                } else if !result.available {
                    entry.last_available_notified = false;
                }
            }
            Err(error) => warn!(error = %error, "failed to poll request availability"),
        }
    }
}

fn poll_active_requests<D>(
    client: &D,
    state: &Arc<Mutex<NotificationState>>,
    writer: &Arc<Mutex<io::Stdout>>,
) where
    D: DaemonRpc,
{
    let active_requests = {
        let state = state.lock().expect("notification state poisoned");
        state
            .active_requests
            .iter()
            .map(|(request_id, wallet_instance_id)| {
                (request_id.clone(), wallet_instance_id.clone())
            })
            .collect::<Vec<_>>()
    };

    for (request_id, wallet_instance_id) in active_requests {
        let request_id =
            starmask_types::RequestId::new(request_id).expect("tracked request id valid");
        match client.get_request_status(GetRequestStatusParams {
            protocol_version: daemon_protocol_version(),
            request_id: request_id.clone(),
        }) {
            Ok(result) => {
                if result.status == RequestStatus::Cancelled {
                    if let Err(error) = emit_notification(
                        writer,
                        &NativeBridgeResponse::RequestCancelled {
                            wallet_instance_id: wallet_instance_id.clone(),
                            request_id: request_id.clone(),
                        },
                    ) {
                        warn!(%error, "failed to emit request.cancelled notification");
                    }
                }

                if result.status.is_terminal() {
                    let mut state = state.lock().expect("notification state poisoned");
                    state.active_requests.remove(request_id.as_str());
                }
            }
            Err(error) => {
                warn!(error = %error, request_id = %request_id, "failed to poll request status")
            }
        }
    }
}

fn emit_notification(
    writer: &Arc<Mutex<io::Stdout>>,
    notification: &NativeBridgeResponse,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(notification)?;
    let mut writer = writer.lock().expect("stdout writer poisoned");
    write_frame(&mut *writer, &payload)
}

#[cfg(test)]
mod tests {
    use super::NotificationState;
    use starmask_types::{
        ClientRequestId, NativeBridgeRequest, NativeBridgeResponse, PayloadHash, PresentationId,
        RequestId, RequestKind, WalletInstanceId,
    };

    #[test]
    fn resumed_request_is_tracked_for_cancel_notifications() {
        let mut state = NotificationState::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let request_id = RequestId::new("request-1").unwrap();

        state.observe(
            &NativeBridgeRequest::RequestPullNext {
                message_id: "msg-1".to_owned(),
                wallet_instance_id: wallet_instance_id.clone(),
            },
            &NativeBridgeResponse::RequestNext {
                reply_to: "msg-1".to_owned(),
                request_id: request_id.clone(),
                client_request_id: ClientRequestId::new("client-1").unwrap(),
                kind: RequestKind::SignMessage,
                account_address: "0x1".to_owned(),
                payload_hash: PayloadHash::new("hash-1").unwrap(),
                display_hint: None,
                client_context: None,
                resume_required: true,
                delivery_lease_id: None,
                lease_expires_at: None,
                presentation_id: Some(PresentationId::new("presentation-1").unwrap()),
                presentation_expires_at: None,
                raw_txn_bcs_hex: None,
                message: Some("hello".to_owned()),
                message_format: Some(starmask_types::MessageFormat::Utf8),
                output_file: None,
                force: false,
                private_key_file: None,
            },
        );

        assert_eq!(
            state.active_requests.get(request_id.as_str()),
            Some(&wallet_instance_id)
        );
    }

    #[test]
    fn rejected_extension_registration_is_not_tracked() {
        let mut state = NotificationState::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();

        state.observe(
            &NativeBridgeRequest::ExtensionRegister {
                message_id: "msg-1".to_owned(),
                protocol_version: 1,
                wallet_instance_id: wallet_instance_id.clone(),
                extension_id: "ext.blocked".to_owned(),
                extension_version: "1.0.0".to_owned(),
                profile_hint: None,
                lock_state: starmask_types::LockState::Unlocked,
                accounts_summary: Vec::new(),
            },
            &NativeBridgeResponse::ExtensionRegistered {
                reply_to: "msg-1".to_owned(),
                wallet_instance_id,
                daemon_protocol_version: 1,
                accepted: false,
            },
        );

        assert!(state.wallets.is_empty());
    }

    #[test]
    fn presented_request_is_tracked_after_ack() {
        let mut state = NotificationState::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let request_id = RequestId::new("request-1").unwrap();

        state.observe(
            &NativeBridgeRequest::RequestPresented {
                message_id: "msg-1".to_owned(),
                wallet_instance_id: wallet_instance_id.clone(),
                request_id: request_id.clone(),
                delivery_lease_id: Some(starmask_types::DeliveryLeaseId::new("lease-1").unwrap()),
                presentation_id: PresentationId::new("presentation-1").unwrap(),
            },
            &NativeBridgeResponse::ExtensionAck {
                reply_to: "msg-1".to_owned(),
            },
        );

        assert_eq!(
            state.active_requests.get(request_id.as_str()),
            Some(&wallet_instance_id)
        );
    }

    #[test]
    fn resolve_ack_removes_tracked_request() {
        let mut state = NotificationState::default();
        let wallet_instance_id = WalletInstanceId::new("wallet-1").unwrap();
        let request_id = RequestId::new("request-1").unwrap();

        state
            .active_requests
            .insert(request_id.to_string(), wallet_instance_id.clone());
        state.observe(
            &NativeBridgeRequest::RequestResolve {
                message_id: "msg-2".to_owned(),
                wallet_instance_id,
                request_id: request_id.clone(),
                presentation_id: PresentationId::new("presentation-1").unwrap(),
                result_kind: starmask_types::ResultKind::SignedMessage,
                signed_txn_bcs_hex: None,
                signature: Some("0xsig".to_owned()),
                created_account_address: None,
                created_account_public_key: None,
                created_account_curve: None,
                created_account_is_default: None,
                created_account_is_locked: None,
                exported_account_address: None,
                exported_account_output_file: None,
                imported_account_address: None,
                imported_account_public_key: None,
                imported_account_curve: None,
                imported_account_is_default: None,
                imported_account_is_locked: None,
            },
            &NativeBridgeResponse::ExtensionAck {
                reply_to: "msg-2".to_owned(),
            },
        );

        assert!(!state.active_requests.contains_key(request_id.as_str()));
    }
}
