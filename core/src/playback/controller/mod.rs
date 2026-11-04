//! Native playback actor. Queue/persistence transitions are serialized, while
//! resolving and decoding run outside the actor. A generation cancels old loads.
mod actions;
mod effect_runtime;
mod equalizer;
mod history;
mod intelligence;
mod load;
mod loudness;
mod parametric;
mod power;
pub(crate) use parametric::DeviceRestoreAuthority;
pub use parametric::{
    DeviceParametricComparison, DeviceParametricStatus, DeviceParametricTarget,
    DeviceParametricView, ParametricApplication, ParametricBindingStatus,
};
mod quality;
mod queue_more;
mod queue_more_tip;
mod remote;
mod skip;
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
pub use queue_more_tip::QueueMoreTipAction;
use state::Actor;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::{mpsc, oneshot, watch};
use types::Reply;
pub use types::{
    FmRestore, IntelligenceUpdate, PlaybackCommand, PlaybackFence, PlaybackSnapshot,
    PlaybackSpeedScope, PlaybackStatus, PrefetchSnapshot, QueueTransport, QueueUpdate,
    RemotePlayback, StartupRestore,
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
    Parametric(Box<parametric::Request>),
    ObserveParametric(parametric::ObserveRequest),
    InspectParametric(parametric::InspectRequest),
    Cancel(String),
    Shutdown,
    Loading {
        generation: u64,
        timeline: super::Timeline,
        authorized_position: f64,
    },
    Loaded {
        generation: u64,
        result: Result<load::Prepared>,
    },
    Quality(quality::QualityReady),
    Prefetched(tick::Prefetched),
    QueueMoreGroups(queue_more::GroupsReady),
    QueueMoreRows(queue_more::RowsReady),
    QueueMoreTip(queue_more_tip::Request),
    PowerPreferenceChanged,
}

pub struct PlaybackController {
    sender: mpsc::UnboundedSender<Message>,
    view: watch::Receiver<PlaybackSnapshot>,
    resolver: Arc<SourceResolver>,
    shutdown: Arc<AtomicBool>,
    relay: Option<Arc<crate::relay::publisher::player::Runtime>>,
    quality_trials: tokio::task::JoinHandle<()>,
}
impl PlaybackController {
    pub(crate) async fn queue_more_tip(
        &self,
        token: &SessionToken,
        action: QueueMoreTipAction,
    ) -> Result<Option<String>> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(Message::QueueMoreTip(queue_more_tip::Request {
                token: token.clone(),
                action,
                reply,
            }))
            .map_err(|_| Error::Cancelled)?;
        receiver.await.map_err(|_| Error::Cancelled)?
    }

    pub fn start(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Box<dyn AudioEngine>,
    ) -> Result<Self> {
        Self::start_inner(session, store, resolver, engine, false, false, None)
    }
    /// Production native observer + durable network worker. The ordinary start
    /// constructor remains isolated for embedders and read-only test adapters.
    pub fn start_with_relay_publication(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Box<dyn AudioEngine>,
    ) -> Result<Self> {
        Self::start_inner(session, store, resolver, engine, true, false, None)
    }
    pub(crate) fn start_inner(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Box<dyn AudioEngine>,
        relay_enabled: bool,
        queue_more_enabled: bool,
        power: Option<Arc<super::keep_awake::PowerControl>>,
    ) -> Result<Self> {
        tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("playback requires an async runtime"))?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut actor = Actor::new(
            session.clone(),
            store,
            resolver.clone(),
            Arc::new(Mutex::new(engine)),
            sender.clone(),
            shutdown.clone(),
        )?;
        actor.queue_more.supported = queue_more_enabled;
        actor.power = power.map(super::keep_awake::PowerEffect::new);
        let relay = if relay_enabled {
            let runtime = crate::relay::publisher::player::Runtime::start(session.clone())?;
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
        let quality_trials = crate::playback::quality::trials::monitor::spawn(session);
        Ok(Self {
            sender,
            view,
            resolver,
            shutdown,
            relay,
            quality_trials,
        })
    }
    pub fn relay_runtime(&self) -> Option<&Arc<crate::relay::publisher::player::Runtime>> {
        self.relay.as_ref()
    }
    pub(crate) fn power_preference_changed(&self) {
        let _ = self.sender.send(Message::PowerPreferenceChanged);
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.view.borrow().clone()
    }
    pub(crate) fn source_resolver(&self) -> &SourceResolver {
        &self.resolver
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

    pub(crate) async fn queue_with_more(
        &self,
        token: &SessionToken,
    ) -> Result<(
        PlaybackSnapshot,
        QueueSnapshot,
        Option<super::queue_more::QueueMoreView>,
    )> {
        let reply = self.request(token, PlaybackCommand::Queue, None).await?;
        Ok((
            reply.state,
            reply.queue.ok_or(Error::State("missing queue reply"))?,
            reply.queue_more,
        ))
    }

    /// Native-only, read-only M/59.j audit for an ordinary song queue. This
    /// does not select a row, change queue state, or obtain a media URL.
    pub async fn ordinary_queue_preflight(
        &self,
        token: &SessionToken,
        expected_queue_revision: u64,
        environment: super::preflight::OrdinaryQueueEnvironment,
    ) -> Result<super::preflight::OrdinaryQueuePreflight> {
        let (before, queue) = self.queue_with_state(token).await?;
        if before.queue_revision != expected_queue_revision {
            return Err(Error::QueueChanged);
        }
        if before.fm || before.intelligence.is_some() {
            return Err(Error::InvalidInput("preflight requires ordinary playback"));
        }
        let result = self
            .resolver
            .ordinary_queue_preflight(token, &queue, environment)
            .await?;
        let (after, current) = self.queue_with_state(token).await?;
        if after.queue_revision != before.queue_revision || current != queue {
            return Err(Error::QueueChanged);
        }
        self.resolver.check_account_generation(token)?;
        Ok(result)
    }
}
impl Drop for PlaybackController {
    fn drop(&mut self) {
        self.quality_trials.abort();
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
