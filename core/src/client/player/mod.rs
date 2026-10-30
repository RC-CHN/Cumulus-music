mod dto;
mod lyrics;
mod next_play;
mod origin;
mod output;
mod preferences;
mod selection;
mod startup;
use super::{MusicClient, mapping};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::LocalLibrary,
    lyrics::LyricsService,
    playback::{
        controller::{PlaybackCommand, PlaybackController, PlaybackSnapshot},
        engine::{AudioEngine, MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    storage::StateStore,
};
pub use dto::*;
pub use output::{OutputCommand, OutputEngineView, OutputView};
pub use preferences::{StartupPreferencesCommand, StartupPreferencesView};
use std::sync::{Arc, Mutex, atomic::AtomicU64};
use tokio::sync::{OnceCell, watch};

type EngineFactory = Arc<dyn Fn() -> Result<Box<dyn AudioEngine>> + Send + Sync>;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeTransportPolicy {
    Ordinary,
    PreserveStep,
    RestartStopped,
}
pub(super) struct PlayerHost {
    relay_enabled: bool,
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    factory: EngineFactory,
    output: Arc<crate::playback::output::OutputRuntime>,
    inspector: OnceCell<Arc<Mutex<Box<dyn AudioEngine>>>>,
    pub(super) controller: OnceCell<Arc<PlaybackController>>,
    fm: OnceCell<Arc<crate::fm::player::FmPlayer>>,
    pub(super) selection: Arc<AtomicU64>,
    focus: Arc<super::focus::MediaFocus>,
    lyrics: Mutex<Option<(SessionToken, Arc<LyricsService>)>>,
}
impl PlayerHost {
    pub fn new(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        focus: Arc<super::focus::MediaFocus>,
        output: Arc<crate::playback::output::OutputRuntime>,
    ) -> Self {
        Self {
            relay_enabled: false,
            session,
            store,
            factory: Arc::new(|| Ok(Box::new(MpvEngine::open(MpvConfig::default())?))),
            controller: OnceCell::new(),
            fm: OnceCell::new(),
            selection: Arc::new(AtomicU64::new(0)),
            lyrics: Mutex::new(None),
            focus,
            output,
            inspector: OnceCell::new(),
        }
    }
    pub(super) async fn controller(&self) -> Result<&Arc<PlaybackController>> {
        self.controller
            .get_or_try_init(|| async {
                let factory = self.factory.clone();
                let lease = self
                    .output
                    .reserve(crate::playback::output::OutputKind::Audio)?;
                let engine = tokio::task::spawn_blocking(move || factory())
                    .await
                    .map_err(|_| Error::State("audio initialization failed"))??;
                let library = Arc::new(LocalLibrary::new(self.store.clone())?);
                let resolver = Arc::new(SourceResolver::new(self.session.clone(), Some(library)));
                let start = if self.relay_enabled {
                    PlaybackController::start_with_relay_publication
                } else {
                    PlaybackController::start
                };
                Ok(Arc::new(start(
                    self.session.clone(),
                    self.store.clone(),
                    resolver,
                    Box::new(lease.wrap(Box::new(super::focus::Focused {
                        inner: engine,
                        focus: self.focus.clone(),
                        owner: super::focus::Owner::Audio,
                    }))),
                )?))
            })
            .await
    }
    pub(super) async fn fm(&self) -> Result<&Arc<crate::fm::player::FmPlayer>> {
        self.fm
            .get_or_try_init(|| async {
                Ok(Arc::new(crate::fm::player::FmPlayer::new(
                    self.session.clone(),
                    self.controller().await?.clone(),
                    self.selection.clone(),
                )?))
            })
            .await
    }
}
/// A native subscription maps each snapshot to a public whitelist and stops
/// when its account changes. No Cookie, raw resource or signed URL is exposed.
pub struct PlaybackSubscription {
    receiver: watch::Receiver<PlaybackSnapshot>,
    session: Arc<AccountSession>,
    token: SessionToken,
}
impl PlaybackSubscription {
    pub fn current(&self) -> Result<PlayerView> {
        self.session.check_generation(&self.token)?;
        project(&self.receiver.borrow(), &self.token)
    }
    pub async fn changed(&mut self) -> Result<PlayerView> {
        self.receiver
            .changed()
            .await
            .map_err(|_| Error::Cancelled)?;
        self.current()
    }
}
pub(super) fn track(resource: &serde_json::Value) -> Result<super::dto::TrackView> {
    if let Some(id) = resource["local_id"].as_str() {
        let metadata = serde_json::from_value(resource["metadata"].clone())?;
        Ok(super::local::view::track(id.into(), &metadata, false))
    } else if resource["song"].is_object() {
        mapping::track(&resource["song"])
    } else {
        let mut result =
            mapping::track(&serde_json::json!({"id":resource["id"],"name":"歌曲信息暂不可用"}))?;
        result.missing = resource["missing"].as_bool().unwrap_or(false);
        Ok(result)
    }
}
pub(super) fn project(value: &PlaybackSnapshot, token: &SessionToken) -> Result<PlayerView> {
    if &value.account != token {
        return Err(Error::StaleResponse);
    }
    Ok(PlayerView {
        fm: value.fm,
        loudness: LoudnessView {
            revision: (!value.loudness.damaged)
                .then(|| value.loudness.preferences.revision.clone()),
            enabled: value.loudness.preferences.enabled,
            status: value.loudness.status,
            gain_db: value
                .loudness
                .gain
                .map(crate::effects::loudness::TrackGain::decibels),
            damaged: value.loudness.damaged,
        },
        equalizer: EqualizerView {
            revision: value.equalizer.preferences.revision.to_string(),
            settings: value.equalizer.preferences.settings,
            status: value.equalizer.status,
            damaged: value.equalizer.damaged,
        },
        epoch: token.generation.to_string(),
        revision: value.revision.to_string(),
        queue_revision: value.queue_revision.to_string(),
        state: value.state,
        play_id: value.play_id.clone(),
        entry_id: value.selected.as_ref().map(|i| i.entry_id.clone()),
        track: value
            .selected
            .as_ref()
            .map(|i| track(&i.resource))
            .transpose()?,
        origin: value
            .selected
            .as_ref()
            .and_then(|i| origin::project(&i.source)),
        position: value.position,
        timeline: value.timeline.clone(),
        requested_quality: value.requested_quality.clone(),
        actual_quality: value.actual_quality.clone(),
        volume: value.volume,
        speed: value.speed,
        mode: value.mode,
        queue_len: value.queue_len,
        error: value
            .error
            .as_ref()
            .map(|_| "播放失败，请刷新音源或重新选择歌曲"),
        position_reset: value.position_reset,
        history_warning: value.history_error.as_ref().map(|_| "最近播放暂未保存"),
    })
}
impl MusicClient {
    /// Enable official same-account publication for a native host before its
    /// player starts. Injected read-only sessions stay isolated unless opted in.
    pub fn with_relay_publication(mut self) -> Self {
        self.player.relay_enabled = true;
        self
    }
    pub fn validate_player_epoch(&self, epoch: &str) -> Result<()> {
        self.player_token(epoch).map(|_| ())
    }
    /// Native host/test injection. Use before any player operation.
    pub fn with_audio_factory(
        mut self,
        factory: impl Fn() -> Result<Box<dyn AudioEngine>> + Send + Sync + 'static,
    ) -> Self {
        self.player.factory = Arc::new(factory);
        self
    }
    pub(super) fn player_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.check_generation(&token)?;
        Ok(token)
    }
    pub async fn subscribe_playback(&self, epoch: &str) -> Result<PlaybackSubscription> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        self.player.fm().await?;
        let mut receiver = controller.subscribe();
        controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        while receiver.borrow().account != token {
            receiver.changed().await.map_err(|_| Error::Cancelled)?;
            self.session.check_generation(&token)?;
        }
        Ok(PlaybackSubscription {
            receiver,
            session: self.session.clone(),
            token,
        })
    }
    pub async fn player_action(&self, epoch: &str, action: PlayerAction) -> Result<PlayerView> {
        self.player_action_checked(epoch, action, None).await
    }
    pub(super) async fn player_action_checked(
        &self,
        epoch: &str,
        action: PlayerAction,
        native: Option<&super::native_media::NativeMediaTarget>,
    ) -> Result<PlayerView> {
        self.player_action_policy(epoch, action, native, NativeTransportPolicy::Ordinary)
            .await
    }
    pub(super) async fn player_action_policy(
        &self,
        epoch: &str,
        action: PlayerAction,
        native: Option<&super::native_media::NativeMediaTarget>,
        policy: NativeTransportPolicy,
    ) -> Result<PlayerView> {
        if let Some(target) = native {
            target.check(self)?;
        }
        if matches!(action, PlayerAction::Startup) {
            return self.startup_player(epoch).await;
        }
        if let PlayerAction::NextPlay {
            id,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .next_play(epoch, id, quality, expected_queue_revision)
                .await;
        }
        let token = self.player_token(epoch)?;
        let local_effect = matches!(
            action,
            PlayerAction::Equalizer { .. }
                | PlayerAction::RetryEqualizer
                | PlayerAction::RepairEqualizer
                | PlayerAction::Loudness { .. }
                | PlayerAction::RetryLoudness
                | PlayerAction::RepairLoudness
        );
        self.together_validate_action(&token, &action)?;
        let together_action = super::together::signal(&action);
        let control_intent = if !matches!(
            action,
            PlayerAction::Volume { .. }
                | PlayerAction::Speed { .. }
                | PlayerAction::Mode { .. }
                | PlayerAction::Equalizer { .. }
                | PlayerAction::RetryEqualizer
                | PlayerAction::RepairEqualizer
                | PlayerAction::Loudness { .. }
                | PlayerAction::RetryLoudness
                | PlayerAction::RepairLoudness
        ) {
            Some(
                self.player
                    .selection
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                    + 1,
            )
        } else {
            None
        };
        let lease = if matches!(
            action,
            PlayerAction::Play
                | PlayerAction::Pause { paused: false }
                | PlayerAction::Next
                | PlayerAction::Previous
                | PlayerAction::Select { .. }
        ) {
            Some(if let Some(target) = native {
                self.claim_native_media(&token, super::focus::Owner::Audio, target.focus)
                    .await?
            } else {
                self.claim_media(&token, super::focus::Owner::Audio).await?
            })
        } else {
            None
        };
        let controller = self.player.controller().await?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if let Some(target) = native {
            target.audio(&current)?;
            target
                .cancellation(self, lease, super::focus::Owner::Audio, control_intent)
                .check()?;
        }
        if current.fm {
            if matches!(
                action,
                PlayerAction::Select { .. }
                    | PlayerAction::Remove { .. }
                    | PlayerAction::Reorder { .. }
                    | PlayerAction::Clear
            ) {
                return Err(Error::State("FM queue requires its coordinator"));
            }
            if matches!(
                action,
                PlayerAction::Next
                    | PlayerAction::Previous
                    | PlayerAction::Mode { .. }
                    | PlayerAction::Play
                    | PlayerAction::Restore
            ) {
                if let Some(lease) = lease {
                    self.focus.check(lease, super::focus::Owner::Audio)?;
                }
                let attempt = control_intent.unwrap_or_else(|| {
                    self.player
                        .selection
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                        + 1
                });
                let fm = self.player.fm().await?;
                let state = match action {
                    PlayerAction::Next => {
                        fm.step_with_state_for(
                            &token,
                            crate::fm::FmStep::ManualNext,
                            attempt,
                            policy == NativeTransportPolicy::PreserveStep,
                        )
                        .await?
                    }
                    PlayerAction::Previous => {
                        fm.step_with_state_for(
                            &token,
                            crate::fm::FmStep::Previous,
                            attempt,
                            policy == NativeTransportPolicy::PreserveStep,
                        )
                        .await?
                    }
                    PlayerAction::Play => {
                        fm.resume_with_start_for(
                            &token,
                            false,
                            attempt,
                            policy == NativeTransportPolicy::RestartStopped,
                        )
                        .await?
                    }
                    PlayerAction::Restore => fm.resume_for(&token, true, attempt).await?,
                    PlayerAction::Mode { mode } => {
                        fm.repeat_for(
                            &token,
                            match mode {
                                crate::playback::PlaybackMode::SingleLoop => {
                                    crate::fm::FmRepeat::One
                                }
                                crate::playback::PlaybackMode::Sequential
                                | crate::playback::PlaybackMode::OrderLoop => {
                                    crate::fm::FmRepeat::Infinite
                                }
                                _ => {
                                    return Err(Error::InvalidInput(
                                        "FM only supports infinite or single repeat",
                                    ));
                                }
                            },
                            attempt,
                        )
                        .await?
                    }
                    _ => unreachable!("FM action checked"),
                };
                return project(&state, &token);
            }
        }
        let command = match action {
            PlayerAction::Startup => unreachable!("handled before user transport commands"),
            PlayerAction::NextPlay { .. } => unreachable!("handled before user transport commands"),
            PlayerAction::Equalizer {
                expected_revision,
                settings,
            } => {
                let revision = expected_revision
                    .parse::<u64>()
                    .map_err(|_| Error::InvalidInput("invalid equalizer revision"))?;
                if revision.to_string() != expected_revision {
                    return Err(Error::InvalidInput("invalid equalizer revision"));
                }
                PlaybackCommand::Equalizer {
                    expected_revision: revision,
                    settings,
                }
            }
            PlayerAction::RetryEqualizer => PlaybackCommand::RetryEqualizer,
            PlayerAction::RepairEqualizer => PlaybackCommand::RepairEqualizer,
            PlayerAction::Loudness {
                expected_revision,
                enabled,
            } => PlaybackCommand::Loudness {
                expected_revision,
                enabled,
            },
            PlayerAction::RetryLoudness => PlaybackCommand::RetryLoudness,
            PlayerAction::RepairLoudness => PlaybackCommand::RepairLoudness,
            PlayerAction::Play if policy == NativeTransportPolicy::RestartStopped => {
                PlaybackCommand::PlayFromStart
            }
            PlayerAction::Play => PlaybackCommand::Play,
            PlayerAction::Pause { paused } => PlaybackCommand::Pause(paused),
            PlayerAction::Seek { seconds } => PlaybackCommand::Seek(seconds),
            PlayerAction::Volume { value } => PlaybackCommand::Volume(value),
            PlayerAction::Speed { value } => PlaybackCommand::Speed(value),
            PlayerAction::Next if policy == NativeTransportPolicy::PreserveStep => {
                PlaybackCommand::StepPreservingState(crate::playback::Step::Next)
            }
            PlayerAction::Previous if policy == NativeTransportPolicy::PreserveStep => {
                PlaybackCommand::StepPreservingState(crate::playback::Step::Previous)
            }
            PlayerAction::Next => PlaybackCommand::Next,
            PlayerAction::Previous => PlaybackCommand::Previous,
            PlayerAction::Stop => PlaybackCommand::Stop,
            PlayerAction::Restore => PlaybackCommand::Restore,
            PlayerAction::Refresh => PlaybackCommand::RefreshSource,
            PlayerAction::Quality { level } => PlaybackCommand::Quality(level),
            PlayerAction::Mode { mode } => PlaybackCommand::Mode(mode),
            PlayerAction::Select { entry_id } => PlaybackCommand::Select(entry_id),
            PlayerAction::Remove { entry_ids } => PlaybackCommand::Remove(entry_ids),
            PlayerAction::Reorder { entry_ids } => PlaybackCommand::Reorder(entry_ids),
            PlayerAction::Clear => PlaybackCommand::Replace {
                resources: vec![],
                index: 0,
                source: serde_json::Value::Null,
            },
        };
        if control_intent.is_some_and(|intent| {
            self.player
                .selection
                .load(std::sync::atomic::Ordering::Acquire)
                != intent
        }) {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller().await?;
        if let Some(lease) = lease {
            self.focus.check(lease, super::focus::Owner::Audio)?;
        }
        let state = if let Some(target) = native {
            controller
                .command_guarded(
                    &token,
                    command,
                    target.audio(&current)?,
                    target.cancellation(self, lease, super::focus::Owner::Audio, control_intent),
                )
                .await?
        } else {
            controller.command(&token, command).await?
        };
        self.session.check_generation(&token)?;
        if !local_effect {
            self.together_signal(&token, together_action);
        }
        project(&state, &token)
    }
    pub async fn player_queue(&self, epoch: &str) -> Result<QueueView> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let (snapshot, queue) = controller.queue_with_state(&token).await?;
        self.session.check_generation(&token)?;
        Ok(QueueView {
            fm: snapshot.fm,
            epoch: epoch.into(),
            revision: snapshot.queue_revision.to_string(),
            current: queue.current,
            tracks: queue
                .items
                .iter()
                .map(|i| {
                    Ok(QueueTrack {
                        entry_id: i.entry_id.clone(),
                        track: track(&i.resource)?,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}
