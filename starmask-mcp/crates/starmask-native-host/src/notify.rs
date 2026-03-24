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
            }
            | NativeBridgeRequest::ExtensionHeartbeat {
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
