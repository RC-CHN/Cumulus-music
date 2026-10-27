//! Host-independent supervision of the blocking native message transport.
//! Only credential reads are retried here. No REST room/player write is replayed.
mod native;
mod worker;
use super::{ImCredentials, Room, native_im::ImEvent};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
};
pub use native::NativeFactory;
use serde::Serialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

pub trait ImConnection: Send {
    fn enter(&mut self, room: &Room) -> Result<()>;
    fn next_event(&mut self, timeout: Duration) -> Result<Option<ImEvent>>;
    fn close(&mut self) -> Result<()>;
}
pub trait ImFactory: Send + Sync {
    /// Called on a blocking worker, never a WebView/runtime event thread.
    fn connect(
        &self,
        session: Arc<AccountSession>,
        token: SessionToken,
        credentials: ImCredentials,
    ) -> Result<Box<dyn ImConnection>>;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Unavailable,
    Idle,
    Connecting,
    Entering,
    Connected,
    Reconnecting,
    Failed,
    Closing,
    Closed,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub state: ConnectionState,
    pub message: Option<String>,
    pub received: u64,
    pub attempt: u32,
}
/// Private to the native use case; not an IPC event. All server bodies remain
/// on the core side of the typed frontend boundary.
pub enum TransportEvent {
    Entered { reconnected: bool },
    Message(ImEvent),
}
pub struct ImTransport {
    stop: Arc<AtomicBool>,
    cancel: watch::Sender<bool>,
    state: watch::Receiver<ConnectionView>,
    completion: tokio::sync::Mutex<Completion>,
}
struct Completion {
    worker: Option<tokio::task::JoinHandle<Result<()>>>,
    succeeded: Option<bool>,
}
pub struct TransportCancellation {
    stop: Arc<AtomicBool>,
    cancel: watch::Sender<bool>,
}
impl Drop for TransportCancellation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.cancel.send_replace(true);
    }
}
impl ImTransport {
    pub fn start(
        factory: Arc<dyn ImFactory>,
        session: Arc<AccountSession>,
        token: SessionToken,
        room: Room,
    ) -> (Self, mpsc::Receiver<TransportEvent>) {
        let stop = Arc::new(AtomicBool::new(false));
        let (cancel, cancelled) = watch::channel(false);
        let (state_tx, state) = watch::channel(ConnectionView {
            state: ConnectionState::Connecting,
            ..Default::default()
        });
        let (sender, receiver) = mpsc::channel(64);
        let worker = tokio::spawn(
            worker::Context {
                factory,
                session,
                token,
                room,
                stop: stop.clone(),
                cancel: cancelled,
                view: state_tx,
                events: sender,
            }
            .run(),
        );
        (
            Self {
                stop,
                cancel,
                state,
                completion: tokio::sync::Mutex::new(Completion {
                    worker: Some(worker),
                    succeeded: None,
                }),
            },
            receiver,
        )
    }
    pub fn view(&self) -> ConnectionView {
        self.state.borrow().clone()
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<ConnectionView> {
        self.state.clone()
    }
    pub fn cancel_on_drop(&self) -> TransportCancellation {
        TransportCancellation {
            stop: self.stop.clone(),
            cancel: self.cancel.clone(),
        }
    }
    /// Shared, cancellation-safe join. An interrupted caller leaves the join
    /// handle in place so another closer still waits for actual SDK cleanup.
    pub async fn close(&self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        self.cancel.send_replace(true);
        let mut completion = self.completion.lock().await;
        if let Some(succeeded) = completion.succeeded {
            return if succeeded {
                Ok(())
            } else {
                Err(Error::State("IM cleanup unresolved"))
            };
        }
        let result = if let Some(worker) = completion.worker.as_mut() {
            match worker.await {
                Ok(result) => result,
                Err(_) => Err(Error::State("IM supervisor failed")),
            }
        } else {
            Err(Error::State("IM supervisor missing"))
        };
        completion.succeeded = Some(result.is_ok());
        completion.worker.take();
        result
    }
}
impl Drop for ImTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.cancel.send_replace(true);
        // Do not abort: the task must join its blocking SDK worker and cleanup.
    }
}
