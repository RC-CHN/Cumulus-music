use super::{
    Engine, Message, PlaybackSnapshot, PlaybackStatus, PrefetchSnapshot, Reply, Response,
    types::Preferences,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{
        PlaybackQueue, QueueEntry, Timeline,
        engine::{Cancellation, LoadedMedia},
        source::{ResolvedSource, SourceResolver},
    },
    storage::StateStore,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

pub(super) struct Loaded {
    pub history: crate::history::playback::Checkpoint,
    pub item: QueueEntry,
    pub source: Arc<ResolvedSource>,
    pub media: LoadedMedia,
    pub timeline: Timeline,
}
pub(super) struct Pending {
    pub history: crate::history::playback::Checkpoint,
    pub id: String,
    pub item: QueueEntry,
    pub reply: Option<Response>,
    pub paused: bool,
}
pub(super) struct Actor {
    pub startup_pending: bool,
    pub startup_autoplay: bool,
    pub relay: Option<crate::relay::publisher::player::Observer>,
    pub relay_hint: crate::relay::publisher::player::Hint,
    pub equalizer: crate::effects::equalizer::EqualizerSnapshot,
    pub equalizer_initialized: bool,
    pub session: Arc<AccountSession>,
    pub store: Arc<StateStore>,
    pub resolver: Arc<SourceResolver>,
    pub engine: Engine,
    pub sender: mpsc::UnboundedSender<Message>,
    pub shutdown: Arc<AtomicBool>,
    pub generation: Arc<AtomicU64>,
    pub token: SessionToken,
    pub bound: bool,
    pub revision: u64,
    pub control_revision: u64,
    pub queue: PlaybackQueue,
    pub prefs: Preferences,
    pub state: PlaybackStatus,
    pub play_id: Option<String>,
    pub position: Option<f64>,
    pub error: Option<String>,
    pub history_error: Option<String>,
    pub reset: bool,
    pub loaded: Option<Loaded>,
    pub pending: Option<Pending>,
    pub load_task: Option<JoinHandle<()>>,
    pub cancel: Cancellation,
    pub quality_task: Option<JoinHandle<()>>,
    pub quality_id: Option<String>,
    pub prefetch_task: Option<JoinHandle<()>>,
    pub prefetch: PrefetchSnapshot,
    pub prefetch_key: Option<super::tick::PrefetchKey>,
    pub refresh_at: Option<std::time::Duration>,
    pub last_save: Instant,
}
impl Actor {
    pub fn new(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        resolver: Arc<SourceResolver>,
        engine: Engine,
        sender: mpsc::UnboundedSender<Message>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self> {
        let token = session.token()?;
        let owner = token.uid.as_deref().unwrap_or("local-guest");
        let queue = PlaybackQueue::new(Some(store.clone()), owner)?;
        let prefs = Self::preferences(&store, owner)?;
        Ok(Self {
            startup_pending: true,
            startup_autoplay: true,
            relay: None,
            relay_hint: Default::default(),
            equalizer: Default::default(),
            equalizer_initialized: false,
            session,
            store,
            resolver,
            engine,
            sender,
            shutdown,
            generation: Arc::new(AtomicU64::new(0)),
            token,
            bound: true,
            revision: 0,
            control_revision: 0,
            queue,
            prefs,
            state: PlaybackStatus::Idle,
            play_id: None,
            position: None,
            error: None,
            history_error: None,
            reset: false,
            loaded: None,
            pending: None,
            load_task: None,
            cancel: Cancellation::default(),
            quality_task: None,
            quality_id: None,
            prefetch_task: None,
            prefetch: PrefetchSnapshot {
                state: "idle",
                entry_id: None,
            },
            prefetch_key: None,
            refresh_at: None,
            last_save: Instant::now(),
        })
    }
    pub fn preferences(store: &StateStore, owner: &str) -> Result<Preferences> {
        let value: Preferences = store
            .get(owner, "playback", "preferences")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        if !value.volume.is_finite()
            || !(0.0..=100.0).contains(&value.volume)
            || !value.speed.is_finite()
            || !(0.25..=4.0).contains(&value.speed)
        {
            return Err(Error::State("invalid persisted player preferences"));
        }
        Ok(value)
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        let selected = self.queue.selected();
        PlaybackSnapshot {
            startup_pending: self.startup_pending,
            startup_autoplay: self.startup_autoplay,
            fm: self.queue.is_fm(),
            equalizer: self.equalizer,
            revision: self.revision,
            control_revision: self.control_revision,
            queue_revision: self.queue.revision(),
            account: self.token.clone(),
            state: self.state,
            paused: self.state == PlaybackStatus::Paused
                || self.pending.as_ref().is_some_and(|pending| pending.paused),
            play_id: self.play_id.clone(),
            requested_quality: selected
                .as_ref()
                .filter(|i| i.resource.get("id").is_some())
                .map(|i| i.resource["level"].as_str().unwrap_or("standard").into()),
            actual_quality: self
                .loaded
                .as_ref()
                .and_then(|l| l.source.actual_quality())
                .map(str::to_owned),
            selected,
            position: self.position,
            timeline: self.loaded.as_ref().map(|l| l.timeline.snapshot()),
            volume: self.prefs.volume,
            speed: self.prefs.speed,
            mode: self.queue.mode(),
            queue_len: self.queue.len(),
            error: self.error.clone(),
            history_error: self.history_error.clone(),
            prefetch: self.prefetch.clone(),
            position_reset: self.reset,
        }
    }
    pub fn reply(&self, reply: Response, result: Result<()>) {
        let _ = reply.send(result.map(|_| Reply {
            state: self.snapshot(),
            queue: None,
        }));
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn guard(&self) -> Cancellation {
        super::cancellation(
            self.session.clone(),
            self.token.clone(),
            self.generation.clone(),
            self.generation(),
            self.shutdown.clone(),
        )
    }
    pub async fn cancel_load(&mut self) -> Result<()> {
        self.cancel.cancel();
        self.generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| Error::State("playback generation exhausted"))?;
        if let Some(task) = self.load_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(pending) = self.pending.take()
            && let Some(reply) = pending.reply
        {
            let _ = reply.send(Err(Error::Cancelled));
        }
        if let Some(task) = self.quality_task.take() {
            task.abort();
        }
        self.quality_id = None;
        self.prefetch_key = None;
        self.refresh_at = None;
        self.prefetch = PrefetchSnapshot {
            state: "idle",
            entry_id: None,
        };
        Ok(())
    }
    pub async fn stop(&mut self, save: bool) -> Result<()> {
        let saved = if save { self.save_position() } else { Ok(()) };
        self.cancel_load().await?;
        // An explicit stop must still work after a session expires.
        let result = super::engine(self.engine.clone(), Cancellation::default(), |engine| {
            engine.stop()
        })
        .await;
        self.loaded = None;
        self.play_id = None;
        self.position = None;
        self.state = PlaybackStatus::Idle;
        self.reset = false;
        result.and(saved)
    }
    pub fn save_position(&mut self) -> Result<()> {
        let (Some(loaded), Some(seconds)) = (&self.loaded, self.position) else {
            return Ok(());
        };
        if !seconds.is_finite() {
            return Err(Error::State("nonfinite playback position"));
        }
        let queue = self.queue.snapshot();
        let selected = self.queue.selected();
        let value = serde_json::to_value(&queue)?;
        self.store.transaction(|tx| {
            if selected.as_ref() != Some(&loaded.item)
                || tx.get(self.queue.owner(), "playback", "queue")? != Some(value)
            {
                return Ok(());
            }
            if self.state == PlaybackStatus::Playing || loaded.history.played_at.is_some() {
                crate::podcast::progress::mark_heard(
                    tx,
                    self.queue.owner(),
                    &loaded.item.resource,
                )?;
                if matches!(
                    loaded.timeline.snapshot().mode,
                    crate::playback::TimelineMode::Full | crate::playback::TimelineMode::FullTrial
                ) {
                    crate::podcast::progress::save(
                        tx,
                        self.queue.owner(),
                        &loaded.item.resource,
                        seconds,
                        self.state == PlaybackStatus::Ended,
                    )?;
                }
            }
            tx.put(
                self.queue.owner(),
                "playback",
                "position",
                &json!({
                    "entry_id":queue.current, "seconds":seconds,
                    "paused":self.state==PlaybackStatus::Paused,
                    "state":self.state, "timeline_version":1
                }),
            )
        })?;
        self.save_history();
        self.last_save = Instant::now();
        Ok(())
    }
    pub async fn run(
        mut self,
        mut receiver: mpsc::UnboundedReceiver<Message>,
        publisher: watch::Sender<PlaybackSnapshot>,
    ) {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while let Some(revision) = self.revision.checked_add(1) {
            self.revision = revision;
            tokio::select! {
                message = receiver.recv() => match message {
                    Some(Message::Request(request)) => self.request(request).await,
                    Some(Message::Cancel(id)) => {
                        if self.pending.as_ref().is_some_and(|p| p.id == id) {
                            let _ = self.stop(false).await;
                        }
                        if self.quality_id.as_ref() == Some(&id) {
                            if let Some(task) = self.quality_task.take() {
                                task.abort();
                            }
                            self.quality_id = None;
                        }
                    },
                    Some(Message::Loading(generation)) => {
                        if generation == self.generation() {
                            self.state = PlaybackStatus::Loading;
                        }
                    },
                    Some(Message::Loaded { generation, result }) => self.loaded(generation,result).await,
                    Some(Message::Quality(value)) => self.quality_ready(value).await,
                    Some(Message::Prefetched(value)) => self.prefetched(value),
                    Some(Message::Shutdown) | None => break,
                },
                _ = interval.tick() => {
                    if let Err(error) = self.tick().await {
                        let _ = self.stop(false).await;
                        self.error = Some(error.to_string());
                        self.state = PlaybackStatus::Error;
                    }
                },
            }
            if self.bound
                && let Some(relay) = &mut self.relay
            {
                relay.observe(
                    &self.token,
                    &self.queue,
                    self.state,
                    self.position,
                    std::mem::take(&mut self.relay_hint),
                );
            }
            publisher.send_replace(self.snapshot());
            if self.shutdown.load(Ordering::Acquire) {
                break;
            }
        }
        let _ = self.stop(true).await;
        if let Some(task) = self.prefetch_task.take() {
            task.abort();
        }
        publisher.send_replace(self.snapshot());
    }
}
