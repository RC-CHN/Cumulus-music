use super::{
    Engine, Message, PlaybackSnapshot, PlaybackSpeedScope, PlaybackStatus, PrefetchSnapshot, Reply,
    Response,
    types::{CommittedSceneVoice, Preferences},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{
        PlaybackQueue, QueueEntry, Timeline,
        engine::{Cancellation, EngineEvent, LoadedMedia},
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
    pub audiobook_skip: Option<crate::podcast::AudiobookIntervals>,
    pub history: crate::history::playback::Checkpoint,
    pub podcast_progress: Option<super::podcast_progress::Capture>,
    pub item: QueueEntry,
    pub source: Arc<ResolvedSource>,
    pub media: LoadedMedia,
    pub local_equalizer: bool,
    pub indexed_flac: bool,
    pub timeline: Timeline,
}
pub(super) struct Pending {
    pub history: crate::history::playback::Checkpoint,
    pub podcast_progress: Option<super::podcast_progress::Capture>,
    pub id: String,
    pub item: QueueEntry,
    pub reply: Option<Response>,
    pub paused: bool,
    pub prefer_search_download: bool,
    pub timeline: Option<Timeline>,
    pub authorized_position: Option<f64>,
    pub seek: Option<(f64, u64, Cancellation)>,
}
/// Last accepted clock, or the authorized initial position after a native
/// open failed. It is never presented as a running decoder or reused for a
/// different selection.
pub(super) struct SourceRetryPoint {
    pub item: QueueEntry,
    pub position: f64,
    pub paused: bool,
    pub history: crate::history::playback::Checkpoint,
}
pub(super) struct Actor {
    pub startup_pending: bool,
    pub chorus_triggers: Option<super::ChorusTriggers>,
    pub startup_autoplay: bool,
    pub relay: Option<crate::relay::publisher::player::Observer>,
    pub relay_hint: crate::relay::publisher::player::Hint,
    pub equalizer: crate::effects::equalizer::EqualizerSnapshot,
    pub equalizer_initialized: bool,
    pub equalizer_source_latch: bool,
    pub local_equalizer_needs_sync: bool,
    pub parametric: super::parametric::State,
    #[cfg(test)]
    pub(super) test_modern: super::modern_effect::TestModernOwner,
    pub loudness: crate::effects::loudness::LoudnessSnapshot,
    pub loudness_initialized: bool,
    pub session: Arc<AccountSession>,
    pub store: Arc<StateStore>,
    pub resolver: Arc<SourceResolver>,
    pub engine: Engine,
    pub sender: mpsc::UnboundedSender<Message>,
    pub shutdown: Arc<AtomicBool>,
    pub generation: Arc<AtomicU64>,
    pub token: SessionToken,
    pub bound: bool,
    pub(super) startup_played: bool,
    pub revision: u64,
    pub control_revision: u64,
    pub seek_revision: u64,
    pub queue: PlaybackQueue,
    pub queue_more: super::queue_more::Coordinator,
    pub queue_more_tip: super::queue_more_tip::Cue,
    pub power: Option<crate::playback::keep_awake::PowerEffect>,
    pub prefs: Preferences,
    pub song_speed_observations: Option<watch::Receiver<super::SongSpeedObservation>>,
    pub applied_speed: Option<f64>,
    pub song_speed_sync_failed: bool,
    pub quality_preferences: crate::playback::quality_preferences::QualityPreferencesSnapshot,
    pub state: PlaybackStatus,
    pub play_id: Option<String>,
    pub position: Option<f64>,
    /// Keep an explicit paused seek as the display/restore clock until resume
    /// or a different load. An acknowledged seek can still be queued in mpv.
    pub paused_seek: bool,
    pub native_seeking: bool,
    pub buffered_through: Option<f64>,
    pub error: Option<crate::playback::failure::PlaybackFailure>,
    pub history_error: Option<String>,
    pub podcast_publisher: Option<super::podcast_progress::Publisher>,
    pub audiobook_skip_sequence: u64,
    pub audiobook_skip_task: Option<JoinHandle<()>>,
    pub reset: bool,
    pub loaded: Option<Loaded>,
    pub source_retry: Option<SourceRetryPoint>,
    pub deferred_events: Vec<EngineEvent>,
    pub pending: Option<Pending>,
    pub load_task: Option<JoinHandle<()>>,
    pub cancel: Cancellation,
    pub quality_task: Option<JoinHandle<()>>,
    pub quality_id: Option<String>,
    pub prefetch_task: Option<JoinHandle<()>>,
    pub ordinary_eof: Option<super::OrdinaryEofAuthority>,
    pub skip_notices: tokio::sync::broadcast::Sender<super::skip_notice::CommittedSkip>,
    pub eof_flight: Option<super::eof::Flight>,
    pub(super) ended_minibar: Option<super::eof::EndedMinibar>,
    pub prefetch: PrefetchSnapshot,
    pub prefetch_key: Option<super::tick::PrefetchKey>,
    pub refresh_at: Option<std::time::Duration>,
    pub last_save: Instant,
}
impl Actor {
    pub(super) fn record_failure(&mut self, error: &Error) {
        self.error = Some(crate::playback::failure::PlaybackFailure::from_error(error));
    }

    pub(super) async fn stop_after_poll_failure(&mut self, error: &Error) {
        use crate::effects::loudness::LoudnessStatus;
        let loudness_failed = self.loudness.status == LoudnessStatus::Failed;
        let _ = self.stop(false).await;
        if loudness_failed && !matches!(error.root(), Error::Cancelled | Error::StaleResponse) {
            // Stop forgets track gain, but still report the processing failure.
            self.loudness.status = LoudnessStatus::Failed;
        }
        self.record_failure(error);
        self.state = PlaybackStatus::Error;
    }

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
        let quality_preferences =
            crate::playback::quality_preferences::QualityPreferencesSnapshot::read(&store, owner)?;
        Ok(Self {
            startup_pending: true,
            chorus_triggers: None,
            startup_autoplay: true,
            relay: None,
            relay_hint: Default::default(),
            equalizer: Default::default(),
            equalizer_initialized: false,
            equalizer_source_latch: false,
            local_equalizer_needs_sync: false,
            parametric: Default::default(),
            #[cfg(test)]
            test_modern: Default::default(),
            loudness: Default::default(),
            loudness_initialized: false,
            session,
            store,
            resolver,
            engine,
            sender,
            shutdown,
            generation: Arc::new(AtomicU64::new(0)),
            token,
            queue_more: Default::default(),
            queue_more_tip: Default::default(),
            power: None,
            bound: true,
            revision: 0,
            control_revision: 0,
            seek_revision: 0,
            queue,
            prefs,
            song_speed_observations: None,
            applied_speed: None,
            song_speed_sync_failed: false,
            quality_preferences,
            state: PlaybackStatus::Idle,
            startup_played: false,
            play_id: None,
            position: None,
            paused_seek: false,
            native_seeking: false,
            buffered_through: None,
            error: None,
            history_error: None,
            podcast_publisher: None,
            audiobook_skip_sequence: 0,
            audiobook_skip_task: None,
            reset: false,
            loaded: None,
            source_retry: None,
            deferred_events: Vec::new(),
            pending: None,
            load_task: None,
            cancel: Cancellation::default(),
            quality_task: None,
            quality_id: None,
            prefetch_task: None,
            ordinary_eof: None,
            skip_notices: super::skip_notice::publisher(),
            eof_flight: None,
            ended_minibar: None,
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
        let mut value: Preferences = store
            .get(owner, "playback", "preferences")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        if !value.volume.is_finite()
            || !(0.0..=100.0).contains(&value.volume)
            || !value.unmute_volume.is_finite()
            || !(0.0..=100.0).contains(&value.unmute_volume)
            || value.unmute_volume == 0.0
            || !value.speed.is_finite()
            || !(0.25..=4.0).contains(&value.speed)
            || value
                .voice_speed
                .is_some_and(|rate| !rate.is_finite() || !(0.25..=4.0).contains(&rate))
        {
            return Err(Error::State("invalid persisted player preferences"));
        }
        if value
            .song_speed
            .is_some_and(|saved| !(5..=20).contains(&saved.custom_tenths))
        {
            value.song_speed = None;
        }
        Ok(value)
    }
    pub(super) fn comparable_device_play(&self) -> Option<&str> {
        (self.loaded.is_some() && self.pending.is_none() && self.state.active())
            .then_some(self.play_id.as_deref())
            .flatten()
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        let selected = self.queue.selected();
        let speed_scope = PlaybackSpeedScope::for_entry(selected.as_ref());
        let (bound_parametric, applied_effect_id) = self.parametric.bound_presentation();
        let device_parametric = self.parametric.device_status(self.comparable_device_play());
        let parametric_processing = self.state == PlaybackStatus::Playing
            && self
                .loaded
                .as_ref()
                .is_some_and(|loaded| self.parametric.processing(loaded.media.entry_id))
            && (matches!(
                bound_parametric,
                Some(super::ParametricBindingStatus::Configured)
            ) || device_parametric
                .as_ref()
                .is_some_and(|device| device.status == super::DeviceParametricStatus::Configured));
        let catalog_file_source = self.search_catalog_file_active();
        PlaybackSnapshot {
            startup_played: self.startup_played,
            startup_pending: self.startup_pending,
            startup_autoplay: self.startup_autoplay,
            queue_more_available: self.queue_more_available(),
            queue_more_tip_eligible: self.queue_more_tip_eligible(),
            fm: self.queue.is_fm(),
            intelligence: self.queue.intelligence_context().cloned(),
            intelligence_operation: self
                .queue
                .intelligence_session()
                .and_then(|s| s.operation_id.clone()),
            intelligence_last_mode: self
                .queue
                .intelligence_session()
                .filter(|session| session.context.active())
                .map(|session| session.last_mode),
            equalizer: crate::effects::equalizer::EqualizerSnapshot {
                source_bypassed: self.loaded.is_some()
                    && self.equalizer.source_bypassed
                    && self.equalizer.status
                        == crate::effects::equalizer::EqualizerStatus::Configured,
                spatial_status: self.equalizer_spatial_status(),
                ..self.equalizer
            },
            bound_parametric,
            applied_effect_id,
            device_parametric,
            parametric_processing,
            loudness: self.loudness.clone(),
            revision: self.revision,
            control_revision: self.control_revision,
            seek_revision: self.seek_revision,
            queue_revision: self.queue.revision(),
            account: self.token.clone(),
            state: self.state,
            paused: self.state == PlaybackStatus::Paused
                || self.pending.as_ref().is_some_and(|pending| pending.paused),
            play_id: self.play_id.clone(),
            ended_minibar_receipt: self.ended_minibar_receipt(selected.as_ref()),
            committed_scene_voice: self
                .loaded
                .as_ref()
                .filter(|loaded| {
                    loaded.item.source["kind"] == "scene_fm"
                        && self.state.active()
                        && self.pending.is_none()
                        && self.error.is_none()
                        && self.play_id.is_some()
                        && selected.as_ref() == Some(&loaded.item)
                })
                .and_then(|loaded| CommittedSceneVoice::from_entry(&loaded.item)),
            quality_preferences: self.quality_preferences.clone(),
            can_change_quality: !catalog_file_source
                && selected
                    .as_ref()
                    .is_some_and(|i| super::quality::online_resource(&i.resource)),
            catalog_file_source,
            search_download_pending: self
                .pending
                .as_ref()
                .is_some_and(|load| load.prefer_search_download),
            quality_preferences_apply: selected
                .as_ref()
                .is_some_and(|i| super::quality::song_resource(&i.resource)),
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
            position: self.position.or_else(|| {
                let (position, _, authority) = self.pending.as_ref()?.seek.as_ref()?;
                (!authority.is_cancelled()).then_some(*position)
            }),
            buffered_through: self.buffered_through.filter(|_| self.state.active()),
            native_seeking: self.state.active() && self.native_seeking,
            timeline: self
                .loaded
                .as_ref()
                .map(|l| l.timeline)
                .or_else(|| self.pending.as_ref()?.timeline)
                .map(|timeline| timeline.snapshot()),
            volume: self.prefs.volume,
            unmute_volume: self.prefs.unmute_volume,
            speed: self
                .applied_speed
                .filter(|_| self.state.active())
                .unwrap_or_else(|| self.effective_speed(speed_scope)),
            speed_scope,
            song_speed: self.prefs.speed,
            song_speed_view: (speed_scope == PlaybackSpeedScope::Song)
                .then(|| self.song_speed_view())
                .flatten(),
            mode: self.queue.mode(),
            queue_len: self.queue.len(),
            visible_queue_len: self.queue.visible_len(),
            error: self.error,
            history_error: self.history_error.clone(),
            prefetch: self.prefetch.clone(),
            position_reset: self.reset,
        }
    }
    pub fn reply(&self, reply: Response, result: Result<()>) {
        let _ = reply.send(result.map(|_| Reply {
            state: self.snapshot(),
            queue: None,
            queue_more: None,
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
        self.eof_flight = None;
        self.ended_minibar = None;
        if let Some(task) = self.audiobook_skip_task.take() {
            task.abort();
        }
        self.cancel.cancel();
        self.paused_seek = false;
        self.native_seeking = false;
        self.deferred_events.clear();
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
        self.power_set(crate::playback::keep_awake::OfficialPlayingState::Stop);
        let saved = if save { self.save_position() } else { Ok(()) };
        self.cancel_load().await?;
        // An explicit stop must still work after a session expires.
        let result = super::engine(self.engine.clone(), Cancellation::default(), |engine| {
            let stopped = engine.stop();
            let cleared = engine.clear_loudness();
            stopped.and(cleared)
        })
        .await;
        let parametric = self.parametric_reset().await;
        self.loaded = None;
        self.source_retry = None;
        self.loudness_forget_track();
        self.play_id = None;
        self.position = None;
        self.buffered_through = None;
        self.state = PlaybackStatus::Idle;
        self.applied_speed = None;
        self.song_speed_sync_failed = false;
        self.reset = false;
        result.and(parametric).and(saved)
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
        let mut pcm = tokio::time::interval(std::time::Duration::from_millis(40));
        self.power_after_event();
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        pcm.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while let Some(revision) = self.revision.checked_add(1) {
            self.revision = revision;
            tokio::select! {
                message = receiver.recv() => match message {
                    Some(Message::Request(request)) => self.request(request).await,
                    Some(Message::Parametric(request)) => self.parametric_request(*request).await,
                    Some(Message::ObserveParametric(request)) => self.parametric_observe(request).await,
                    Some(Message::InspectParametric(request)) => self.parametric_inspect(request),
                    Some(Message::InspectModernEffect(request)) => {
                        let source = self.modern_effect_source(&request).await;
                        let _ = request.reply.send(source);
                    }
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
                    Some(Message::Loading { generation, timeline, authorized_position }) => {
                        if generation == self.generation() && let Some(pending) = &mut self.pending {
                            pending.timeline = Some(timeline);
                            pending.authorized_position = Some(authorized_position);
                            self.state = PlaybackStatus::Loading;
                            self.power_pending();
                        }
                    },
                    Some(Message::SearchEqualizerHandoff { generation, accepted }) => {
                        if generation == self.generation()
                            && self.pending.is_some()
                            && !self.cancel.is_cancelled()
                        {
                            // Mark the stock filter dirty before a checked
                            // publication may displace it on the engine thread.
                            self.local_equalizer_needs_sync = true;
                            let _ = accepted.send(());
                        }
                    },
                    Some(Message::Loaded { generation, result }) => self.loaded(generation,result).await,
                    Some(Message::AudiobookSkipChanged { token, radio }) => self.refresh_audiobook_skip(token, radio),
                    Some(Message::AudiobookSkipRefreshed(value)) => self.refreshed_audiobook_skip(value),
                    Some(Message::Quality(value)) => self.quality_ready(value).await,
                    Some(Message::Prefetched(value)) => self.prefetched(value),
                    Some(Message::OrdinaryEof(value)) => {
                        if let Err(error) = self.finish_ordinary_eof(*value).await {
                            self.stop_after_poll_failure(&error).await;
                        }
                    },
                    Some(Message::QueueMoreGroups(value)) => self.queue_more_groups(value),
                    Some(Message::QueueMoreRows(value)) => self.queue_more_rows(value).await,
                    Some(Message::QueueMoreTip(request)) => self.queue_more_tip_request(request).await,
                    Some(Message::PowerPreferenceChanged) => {},
                    Some(Message::Shutdown) | None => break,
                },
                _ = super::song_speed::changed(&mut self.song_speed_observations) => {
                    self.song_speed_apply_observation().await;
                },
                _ = interval.tick() => {
                    if let Err(error) = self.tick().await {
                        self.stop_after_poll_failure(&error).await;
                    }
                },
                _ = pcm.tick(), if matches!(self.state, PlaybackStatus::Playing | PlaybackStatus::Buffering)
                    && self.loaded.as_ref().is_some_and(|loaded| loaded.local_equalizer) => {
                    let result = super::engine(self.engine.clone(), self.guard(), |engine| {
                        engine.pump_local_equalizer()
                    }).await;
                    if let Err(error) = result {
                        self.stop_after_poll_failure(&error).await;
                    } else {
                        // Credit sampling has no queue/DB/UI side effects.
                        continue;
                    }
                }
            }
            self.power_after_event();
            self.observe_queue_more();
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
        self.podcast_publisher = None;
        let _ = self.stop(true).await;
        if let Some(task) = self.prefetch_task.take() {
            task.abort();
        }
        self.queue_more.abort();
        publisher.send_replace(self.snapshot());
    }
}
