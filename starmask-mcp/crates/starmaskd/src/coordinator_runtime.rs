use std::{
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use tokio::sync::oneshot;
use uuid::Uuid;

use starmask_core::{
    AllowAllPolicy, Clock, Coordinator, CoordinatorCommand, CoordinatorConfig, CoordinatorResponse,
    CoreResult, IdGenerator, Store,
};
use starmask_types::{DeliveryLeaseId, RequestId, TimestampMs};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> TimestampMs {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        TimestampMs::from_millis(i64::try_from(millis).unwrap_or(i64::MAX))
    }
}

#[derive(Default)]
pub struct UuidIdGenerator;

impl IdGenerator for UuidIdGenerator {
    fn new_request_id(&mut self) -> CoreResult<RequestId> {
        RequestId::new(Uuid::now_v7().to_string())
            .map_err(|error| starmask_core::CoreError::Invariant(error.to_string()))
    }

    fn new_delivery_lease_id(&mut self) -> CoreResult<DeliveryLeaseId> {
        DeliveryLeaseId::new(Uuid::now_v7().to_string())
            .map_err(|error| starmask_core::CoreError::Invariant(error.to_string()))
    }
}

struct CoordinatorEnvelope {
    command: CoordinatorCommand,
    reply: oneshot::Sender<CoreResult<CoordinatorResponse>>,
}

#[derive(Clone)]
pub struct CoordinatorHandle {
    sender: mpsc::Sender<CoordinatorEnvelope>,
}

impl CoordinatorHandle {
    pub async fn dispatch(&self, command: CoordinatorCommand) -> Result<CoordinatorResponse> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(CoordinatorEnvelope {
                command,
                reply: reply_tx,
            })
            .map_err(|error| anyhow!("coordinator is not running: {error}"))?;
        reply_rx
            .await
            .context("coordinator dropped response channel")?
            .map_err(anyhow::Error::from)
    }
}

pub fn spawn_coordinator<S>(store: S, config: CoordinatorConfig) -> CoordinatorHandle
where
    S: Store + Send + 'static,
{
    let (sender, receiver) = mpsc::channel::<CoordinatorEnvelope>();
    thread::spawn(move || {
        let mut coordinator =
            Coordinator::new(store, AllowAllPolicy, SystemClock, UuidIdGenerator, config);
        while let Ok(envelope) = receiver.recv() {
            let result = coordinator.dispatch(envelope.command);
            let _ = envelope.reply.send(result);
        }
    });
    CoordinatorHandle { sender }
}
