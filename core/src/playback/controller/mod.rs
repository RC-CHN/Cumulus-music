//! Native playback actor. Queue/persistence transitions are serialized, while
//! resolving and decoding run outside the actor. A generation cancels old loads.
mod actions;
mod equalizer;
mod history;
mod load;
mod remote;
mod startup;
mod state;
mod tick;
mod types;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{
        QueueSnapshot,
        engine::{AudioEngine, Cancellation},
        source::SourceResolver,
    },
    storage::StateStore,
};
use state::Actor;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::{mpsc, oneshot, watch};
use types::Reply;
pub use types::{
    PlaybackCommand, PlaybackFence, PlaybackSnapshot, PlaybackStatus, PrefetchSnapshot,
    QueueTransport, QueueUpdate, RemotePlayback, StartupRestore,
};

type Engine = Arc<Mutex<Box<dyn AudioEngine>>>;
type Response = oneshot::Sender<Result<Reply>>;
struct Request {
    id: String,
    token: SessionToken,
    command: PlaybackCommand,
    reply: Response,
    guard: Option<(PlaybackFence, Cancellation)>,
}
enum Message {
    Request(Request),
    Cancel(String),
    Shutdown,
    Loading(u64),
    Loaded {
        generation: u64,
        result: Result<load::Prepared>,
    },
    Quality(load::QualityReady),
    Prefetched(tick::Prefetched),
}

pub struct PlaybackController {
    sender: mpsc::UnboundedSender<Message>,
    view: watch::Receiver<PlaybackSnapshot>,
    shutdown: Arc<AtomicBool>,
    relay: Option<Arc<crate::relay::publisher::player::Runtime>>,
}
impl PlaybackController {
    pub fn start(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Box<dyn AudioEngine>,
    ) -> Result<Self> {
        Self::start_inner(session, store, resolver, engine, false)
    }
    /// Production native observer + durable network worker. The ordinary start
    /// constructor remains isolated for embedders and read-only test adapters.
    pub fn start_with_relay_publication(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Box<dyn AudioEngine>,
    ) -> Result<Self> {
        Self::start_inner(session, store, resolver, engine, true)
    }
    fn start_inner(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Box<dyn AudioEngine>,
        relay_enabled: bool,
    ) -> Result<Self> {
        tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("playback requires an async runtime"))?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut actor = Actor::new(
            session.clone(),
            store,
            resolver,
            Arc::new(Mutex::new(engine)),
            sender.clone(),
            shutdown.clone(),
        )?;
        let relay = if relay_enabled {
            let runtime = crate::relay::publisher::player::Runtime::start(session)?;
            actor.relay = Some(crate::relay::publisher::player::Observer::new(
                runtime.clone(),
                actor.token.clone(),
                &actor.queue,
            )?);
            Some(runtime)
        } else {
            None
        };
        let (publisher, view) = watch::channel(actor.snapshot());
        tokio::spawn(actor.run(receiver, publisher));
        Ok(Self {
            sender,
            view,
            shutdown,
            relay,
        })
    }
    pub fn relay_runtime(&self) -> Option<&Arc<crate::relay::publisher::player::Runtime>> {
        self.relay.as_ref()
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.view.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<PlaybackSnapshot> {
        self.view.clone()
    }
    async fn request(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        guard: Option<(PlaybackFence, Cancellation)>,
    ) -> Result<Reply> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(Message::Request(Request {
                id: id.clone(),
                token: token.clone(),
                command,
                reply,
                guard,
            }))
            .map_err(|_| Error::State("player is closed"))?;
        struct CancelOnDrop {
            sender: mpsc::UnboundedSender<Message>,
            id: Option<String>,
        }
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                if let Some(id) = self.id.take() {
                    let _ = self.sender.send(Message::Cancel(id));
                }
            }
        }
        let mut guard = CancelOnDrop {
            sender: self.sender.clone(),
            id: Some(id),
        };
        let result = receiver.await.map_err(|_| Error::Cancelled)?;
        guard.id = None;
        result
    }
    pub async fn command(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
    ) -> Result<PlaybackSnapshot> {
        Ok(self.request(token, command, None).await?.state)
    }
    pub(crate) async fn command_guarded(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        expected: PlaybackFence,
        cancel: Cancellation,
    ) -> Result<PlaybackSnapshot> {
        Ok(self
            .request(token, command, Some((expected, cancel)))
            .await?
            .state)
    }
    pub async fn queue(&self, token: &SessionToken) -> Result<QueueSnapshot> {
        Ok(self.queue_with_state(token).await?.1)
    }
    pub async fn queue_with_state(
        &self,
        token: &SessionToken,
    ) -> Result<(PlaybackSnapshot, QueueSnapshot)> {
        let reply = self.request(token, PlaybackCommand::Queue, None).await?;
        Ok((
            reply.state,
            reply.queue.ok_or(Error::State("missing queue reply"))?,
        ))
    }
}
impl Drop for PlaybackController {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.sender.send(Message::Shutdown);
    }
}

async fn engine<T: Send + 'static>(
    engine: Engine,
    cancel: Cancellation,
    operation: impl FnOnce(&mut dyn AudioEngine) -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        let mut engine = engine.lock().map_err(|_| Error::LockPoisoned)?;
        cancel.check()?;
        let result = operation(engine.as_mut());
        if cancel.is_cancelled() {
            let _ = engine.stop();
            return Err(Error::Cancelled);
        }
        result
    })
    .await
    .map_err(|_| Error::State("audio worker failed"))?
}

fn cancellation(
    session: Arc<AccountSession>,
    token: SessionToken,
    generation: Arc<AtomicU64>,
    expected: u64,
    shutdown: Arc<AtomicBool>,
) -> Cancellation {
    Cancellation::new(move || {
        !shutdown.load(Ordering::Acquire)
            && generation.load(Ordering::Acquire) == expected
            && session.token().is_ok_and(|current| current == token)
    })
}
