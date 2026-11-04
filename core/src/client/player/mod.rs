mod chorus;
mod comment_count;
mod dto;
mod identity;
mod intelligence;
mod lyrics;
mod missing_download;
mod mode;
mod next_play;
mod origin;
mod output;
mod playlist_batch;
mod preferences;
mod prepared_selection;
mod quality;
mod queue_click;
mod queue_collection;
mod queue_drop;
mod queue_edit;
mod queue_playlist;
mod reports;
mod selection;
mod source_control;
mod startup;
use super::{MusicClient, mapping};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::LocalLibrary,
    lyrics::LyricsService,
    playback::{
        controller::{PlaybackCommand, PlaybackController, PlaybackSnapshot, PlaybackStatus},
        engine::{AudioEngine, Cancellation, MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    storage::StateStore,
};
pub use dto::*;
pub use intelligence::{IntelligenceCommand, IntelligenceEligibility, IntelligenceOrigin};
pub use output::{OutputCommand, OutputEngineView, OutputView};
pub use preferences::{StartupPreferencesCommand, StartupPreferencesView};
pub use quality::{
    QualityChoiceView, QualityDetailsView, QualityTrialCommand, QualityTrialOutcome,
    QualityTrialView,
};
pub use queue_click::{QueueClickPreferencesCommand, QueueClickPreferencesView};
pub use reports::LyricReportView;
use std::sync::{Arc, Mutex, atomic::AtomicU64};
use tokio::sync::{OnceCell, watch};

fn validate_quality_target(
    current: &PlaybackSnapshot,
    play_id: &str,
    expected_revision: &str,
) -> Result<()> {
    if current.play_id.as_deref() != Some(play_id)
        || (current.quality_preferences_apply
            && (current.quality_preferences.damaged
                || current.quality_preferences.preferences.revision != expected_revision))
    {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

pub(super) type EngineFactory = Arc<dyn Fn() -> Result<Box<dyn AudioEngine>> + Send + Sync>;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeTransportPolicy {
    Ordinary,
    PreserveStep,
    RestartStopped,
}
pub(super) struct PlayerHost {
    intelligence: Arc<intelligence::IntelligenceHost>,
    relay_enabled: bool,
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    pub(super) factory: EngineFactory,
    output: Arc<crate::playback::output::OutputRuntime>,
    inspector: OnceCell<Arc<Mutex<Box<dyn AudioEngine>>>>,
    pub(super) controller: OnceCell<Arc<PlaybackController>>,
    fm: OnceCell<Arc<crate::fm::player::FmPlayer>>,
    pub(super) selection: Arc<AtomicU64>,
    prepared_selection: Mutex<Option<prepared_selection::PreparedSelection>>,
    prepared_generation: Arc<AtomicU64>,
    focus: Arc<super::focus::MediaFocus>,
    lyrics: Mutex<Option<lyrics::ActiveLyrics>>,
}
impl PlayerHost {
    pub fn new(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        focus: Arc<super::focus::MediaFocus>,
        output: Arc<crate::playback::output::OutputRuntime>,
    ) -> Self {
        Self {
            intelligence: Arc::new(intelligence::IntelligenceHost::default()),
            relay_enabled: false,
            session,
            store,
            factory: Arc::new(|| Ok(Box::new(MpvEngine::open(MpvConfig::default())?))),
            controller: OnceCell::new(),
            fm: OnceCell::new(),
            selection: Arc::new(AtomicU64::new(0)),
            prepared_selection: Mutex::new(None),
            prepared_generation: Arc::new(AtomicU64::new(0)),
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
    let failure = value.error.map(super::ClientError::from);
    Ok(PlayerView {
        fm: value.fm,
        intelligence: value.intelligence.as_ref().map(|state| state.phase),
        intelligence_operation: value.intelligence_operation.clone(),
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
            preset: value.equalizer.preferences.preset,
            saved_custom: value.equalizer.preferences.saved_custom.map(|s| *s.gains()),
            status: value.equalizer.status,
            damaged: value.equalizer.damaged,
        },
        bound_parametric: value.bound_parametric,
        parametric_processing: value.parametric_processing,
        device_parametric: value.device_parametric.clone(),
        epoch: token.generation.to_string(),
        revision: value.revision.to_string(),
        queue_revision: value.queue_revision.to_string(),
        state: value.state,
        pending_paused: matches!(
            value.state,
            PlaybackStatus::Preparing | PlaybackStatus::Loading
        )
        .then_some(value.paused),
        play_id: value.play_id.clone(),
        entry_id: value.selected.as_ref().map(|i| i.entry_id.clone()),
        download_task_id: value
            .selected
            .as_ref()
            .and_then(|i| i.resource["download_task_id"].as_str())
            .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(str::to_owned),
        resource_kind: value.selected.as_ref().map(|i| identity::kind(&i.resource)),
        track: value
            .selected
            .as_ref()
            .map(|i| track(&i.resource))
            .transpose()?,
        origin: value
            .selected
            .as_ref()
            .and_then(|i| origin::project(&i.source)),
        profile_rank_playlist_uid: value
            .selected
            .as_ref()
            .and_then(|entry| crate::playback::profile_rank::uid(&entry.source))
            .map(str::to_owned),
        recommendation_origin: value
            .selected
            .as_ref()
            .and_then(|i| origin::recommendation(&i.source)),
        position: value.position,
        buffered_through: value.buffered_through,
        native_seeking: value.native_seeking,
        timeline: value.timeline.clone(),
        quality_preferences: QualityPreferencesView {
            applies_to_current: value.quality_preferences_apply,
            revision: (!value.quality_preferences.damaged)
                .then(|| value.quality_preferences.preferences.revision.clone()),
            quality: value.quality_preferences.preferences.quality,
            damaged: value.quality_preferences.damaged,
        },
        can_change_quality: value.can_change_quality,
        requested_quality: value.requested_quality.clone(),
        actual_quality: value.actual_quality.clone(),
        volume: value.volume,
        unmute_volume: value.unmute_volume,
        speed: value.speed,
        speed_scope: value.speed_scope,
        mode: value.mode,
        queue_len: value.visible_queue_len,
        error: failure.as_ref().map(|error| error.message),
        error_code: failure.as_ref().map(|error| error.code),
        error_retryable: failure.map(|error| error.retryable),
        position_reset: value.position_reset,
        history_warning: value.history_error.as_ref().map(|_| "最近播放暂未保存"),
    })
}
impl MusicClient {
    pub async fn player_snapshot(&self, epoch: &str) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let value = self
            .player
            .controller()
            .await?
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
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
    /// Trusted native host only. The caller must supply a validated component
    /// whose use is authorized; this method neither acquires nor activates
    /// upstream rights and is not available through renderer IPC.
    pub async fn set_native_parametric_component(
        &self,
        epoch: &str,
        plan: Option<crate::effects::parametric::ParametricPlan>,
    ) -> Result<crate::playback::controller::ParametricApplication> {
        let token = self.player_token(epoch)?;
        let outcome = self
            .player
            .controller()
            .await?
            .set_parametric_component(&token, plan)
            .await?;
        self.session.check_generation(&token)?;
        Ok(outcome)
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
        if let PlayerAction::PlayMissingDownloadOnline {
            play_id,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "online download fallback needs an explicit action",
                ));
            }
            return self
                .play_missing_download_online(epoch, &play_id, &expected_queue_revision)
                .await;
        }
        if let PlayerAction::StepLoading {
            play_id,
            expected_queue_revision,
            direction,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "loading step used as native media command",
                ));
            }
            return self
                .step_loading_source(epoch, &play_id, &expected_queue_revision, direction)
                .await;
        }
        if let PlayerAction::ModeFor {
            mode,
            expected_queue_revision,
            fm,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "popup mode target used as native media command",
                ));
            }
            return self
                .player_mode_for(epoch, mode, &expected_queue_revision, fm)
                .await;
        }
        let next_origin = if matches!(action, PlayerAction::SearchNextPlay { .. }) {
            next_play::NextPlayOrigin::Search
        } else {
            next_play::NextPlayOrigin::Song
        };
        if let PlayerAction::NextPlay {
            id,
            quality,
            expected_queue_revision,
        }
        | PlayerAction::SearchNextPlay {
            id,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .next_play(epoch, id, quality, expected_queue_revision, next_origin)
                .await;
        }
        use queue_drop::{DropSource, DropTarget};
        match action {
            PlayerAction::DropLocalIntoQueue {
                query,
                revision,
                track_id,
                expected_queue_revision,
                expected_play_id,
                before_entry_id,
            } => {
                return self
                    .drop_into_queue(
                        epoch,
                        DropSource::Local {
                            query,
                            revision,
                            track_id,
                        },
                        DropTarget {
                            expected_queue_revision,
                            expected_play_id,
                            before_entry_id,
                        },
                    )
                    .await;
            }
            PlayerAction::DropPlaylistSongIntoQueue {
                id,
                revision,
                track_id,
                track_index,
                quality,
                expected_queue_revision,
                expected_play_id,
                before_entry_id,
            } => {
                return self
                    .drop_into_queue(
                        epoch,
                        DropSource::Playlist {
                            id,
                            revision,
                            rows: vec![PlaylistDropRow {
                                track_id,
                                track_index,
                            }],
                            quality,
                        },
                        DropTarget {
                            expected_queue_revision,
                            expected_play_id,
                            before_entry_id,
                        },
                    )
                    .await;
            }
            PlayerAction::DropPlaylistSongsIntoQueue {
                id,
                revision,
                rows,
                quality,
                expected_queue_revision,
                expected_play_id,
                before_entry_id,
            } => {
                return self
                    .drop_into_queue(
                        epoch,
                        DropSource::Playlist {
                            id,
                            revision,
                            rows,
                            quality,
                        },
                        DropTarget {
                            expected_queue_revision,
                            expected_play_id,
                            before_entry_id,
                        },
                    )
                    .await;
            }
            _ => {}
        }
        if let PlayerAction::QueueLocal {
            query,
            revision,
            track_id,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Local {
                        query,
                        revision,
                        track_id,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueDownloads {
            task_ids,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Downloads(task_ids),
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueuePlaylist {
            id,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Playlist {
                        id,
                        quality,
                        selection: None,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueVoice {
            id,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Voice { id },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueDailyRecommendations {
            revision,
            indices,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "daily queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::DailyRecommendations {
                        revision,
                        indices,
                        quality,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueStyleRecommendations {
            revision,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "style queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::StyleRecommendations { revision, quality },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueArtistSongs {
            pages,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "artist queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::ArtistSongs { pages, quality },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueAlbumBatch {
            id,
            ids,
            revision,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "album batch queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::AlbumBatch {
                        id,
                        ids,
                        revision,
                        quality,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueAlbum {
            id,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Album { id, quality },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueuePlaylistBatch {
            id,
            ids,
            revision,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Playlist {
                        id,
                        quality,
                        selection: Some((ids, revision)),
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueCloudBatch {
            ids,
            revision,
            quality,
            expected_queue_revision,
            query,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Cloud {
                        ids,
                        revision,
                        quality,
                        query,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        let (action, source_target) = match action {
            PlayerAction::PauseFor {
                play_id,
                expected_queue_revision,
                paused,
            } => {
                if native.is_some() {
                    return Err(Error::InvalidInput(
                        "source control used as native media command",
                    ));
                }
                (
                    PlayerAction::Pause { paused },
                    Some((play_id, expected_queue_revision)),
                )
            }
            PlayerAction::SeekFor {
                play_id,
                expected_queue_revision,
                seconds,
            } => {
                if native.is_some() {
                    return Err(Error::InvalidInput(
                        "source control used as native media command",
                    ));
                }
                (
                    PlayerAction::Seek { seconds },
                    Some((play_id, expected_queue_revision)),
                )
            }
            PlayerAction::RefreshFor {
                play_id,
                expected_queue_revision,
            } => {
                if native.is_some() {
                    return Err(Error::InvalidInput(
                        "source control used as native media command",
                    ));
                }
                (
                    PlayerAction::Refresh,
                    Some((play_id, expected_queue_revision)),
                )
            }
            other => (other, None),
        };
        let (mut action, displayed_queue, clear_room) = match action {
            PlayerAction::QueueFor {
                expected_queue_revision,
                expected_room_id,
                edit,
            } => {
                if native.is_some() {
                    return Err(Error::InvalidInput(
                        "displayed queue edit used as native media command",
                    ));
                }
                let room = matches!(edit, QueueEdit::Clear).then_some(expected_room_id);
                (edit.into_action(), Some(expected_queue_revision), room)
            }
            other => (other, None, None),
        };
        let token = self.player_token(epoch)?;
        // A stale card must not cancel another selection or claim media focus.
        let source_guard = if let Some((play_id, revision)) = &source_target {
            let selection = self.player.selection.clone();
            let observed = selection.load(std::sync::atomic::Ordering::Acquire);
            let controller = self.player.controller().await?;
            let current = controller
                .command(&token, PlaybackCommand::Snapshot)
                .await?;
            let pending_action = match (current.state, &action) {
                (
                    PlaybackStatus::Preparing | PlaybackStatus::Loading,
                    PlayerAction::Pause { paused },
                ) => Some(PlaybackCommand::Pause(*paused)),
                (PlaybackStatus::Loading, PlayerAction::Seek { seconds }) => {
                    Some(PlaybackCommand::SeekDuringLoad(*seconds))
                }
                _ => None,
            };
            if let Some(command) = pending_action {
                source_control::validate_identity(&current, play_id, revision)?;
                if current.fm || self.together_attached(&token, None)? {
                    return Err(Error::State("loading controls require ordinary playback"));
                }
                let focus = self.focus.clone();
                let owner = focus.snapshot()?;
                if owner.owner != Some(super::focus::Owner::Audio) {
                    return Err(Error::StaleResponse);
                }
                let room = self.together_detached_guard(&token);
                // This control belongs to the in-flight selection. It must
                // not reserve a new selection or cancel its decoder.
                let state = controller
                    .command_guarded(
                        &token,
                        command,
                        current.fence(),
                        Cancellation::new(move || {
                            selection.load(std::sync::atomic::Ordering::Acquire) == observed
                                && focus.check_snapshot(owner).is_ok()
                                && !room.is_cancelled()
                        }),
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            source_control::validate(
                &current,
                play_id,
                revision,
                matches!(action, PlayerAction::Refresh),
            )?;
            Some(current.fence())
        } else {
            None
        };
        // Keep room attachment from changing the policy of a displayed edit.
        let _queue_room = if displayed_queue.is_some() {
            Some(
                self.together
                    .gate
                    .try_lock()
                    .map_err(|_| Error::MutationBusy)?,
            )
        } else {
            None
        };
        // Reject stale UI before incrementing selection/cancelling a load.
        let queue_guard = if let Some(revision) = displayed_queue {
            Some(
                self.displayed_queue_fence(&token, &revision, &mut action)
                    .await?,
            )
        } else {
            None
        };
        if let Some(room) = clear_room {
            self.validate_queue_clear_room(&token, room.as_deref())?;
        }
        let local_effect = matches!(
            action,
            PlayerAction::Equalizer { .. }
                | PlayerAction::SaveCustomEqualizer { .. }
                | PlayerAction::RetryEqualizer
                | PlayerAction::RepairEqualizer
                | PlayerAction::Loudness { .. }
                | PlayerAction::RetryLoudness
                | PlayerAction::RepairLoudness
                | PlayerAction::RepairQuality
        );
        self.together_validate_action(&token, &action)?;
        let together_action = super::together::signal(&action);
        // Reject obsolete popup input before it can supersede another pending
        // selection. The actor fence below still covers the later race.
        if let PlayerAction::QualityFor {
            play_id,
            expected_revision,
            ..
        } = &action
        {
            let controller = self.player.controller().await?;
            let current = controller
                .command(&token, PlaybackCommand::Snapshot)
                .await?;
            validate_quality_target(&current, play_id, expected_revision)?;
        }
        let fm_transport = if matches!(
            action,
            PlayerAction::Next
                | PlayerAction::Previous
                | PlayerAction::Play
                | PlayerAction::Restore
        ) && self.player.controller.get().is_some_and(|controller| {
            let state = controller.snapshot();
            state.account == token && state.fm
        }) {
            Some(self.player.fm().await?)
        } else {
            None
        };
        let fm_operation = fm_transport.as_ref().map(|fm| fm.operation()).transpose()?;
        let control_intent = if !matches!(
            action,
            PlayerAction::Volume { .. }
                | PlayerAction::Speed { .. }
                | PlayerAction::SpeedFor { .. }
                | PlayerAction::Mode { .. }
                | PlayerAction::Equalizer { .. }
                | PlayerAction::SaveCustomEqualizer { .. }
                | PlayerAction::RetryEqualizer
                | PlayerAction::RepairEqualizer
                | PlayerAction::Loudness { .. }
                | PlayerAction::RetryLoudness
                | PlayerAction::RepairLoudness
                | PlayerAction::RepairQuality
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
        let source_resume =
            source_guard.is_some() && matches!(action, PlayerAction::Pause { paused: false });
        let lease = if source_resume {
            Some(self.audio_source_resume_lease(&token).await?)
        } else if matches!(
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
        if queue_guard.as_ref().is_some_and(|f| *f != current.fence()) {
            return Err(Error::QueueChanged);
        }
        if source_guard.as_ref().is_some_and(|f| *f != current.fence()) {
            return Err(Error::StaleResponse);
        }
        if matches!(action, PlayerAction::Mode { .. })
            && current
                .intelligence
                .as_ref()
                .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
            && self.focus.snapshot()?.owner != Some(super::focus::Owner::Audio)
        {
            return Err(Error::State("resume audio before leaving intelligence"));
        }
        if let Some(target) = native {
            target.audio(&current)?;
            target
                .cancellation(self, lease, super::focus::Owner::Audio, control_intent)
                .check()?;
        }
        let quality_guard = if let PlayerAction::QualityFor {
            play_id,
            expected_revision,
            ..
        } = &action
        {
            validate_quality_target(&current, play_id, expected_revision)?;
            Some(current.fence())
        } else {
            None
        };
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
                // Mode changes must not supersede an accepted FM step when
                // its coordinator is busy. Transport already owns an intent;
                // mode only observes it and retains the exact actor fence.
                let attempt = control_intent.unwrap_or_else(|| {
                    self.player
                        .selection
                        .load(std::sync::atomic::Ordering::Acquire)
                });
                let fm = self.player.fm().await?;
                let state = match action {
                    PlayerAction::Next => {
                        if let Some(operation) = fm_operation.as_ref() {
                            operation
                                .step(
                                    &token,
                                    crate::fm::FmStep::ManualNext,
                                    attempt,
                                    policy == NativeTransportPolicy::PreserveStep,
                                )
                                .await?
                        } else {
                            fm.step_with_state_for(
                                &token,
                                crate::fm::FmStep::ManualNext,
                                attempt,
                                policy == NativeTransportPolicy::PreserveStep,
                            )
                            .await?
                        }
                    }
                    PlayerAction::Previous => {
                        if let Some(operation) = fm_operation.as_ref() {
                            operation
                                .step(
                                    &token,
                                    crate::fm::FmStep::Previous,
                                    attempt,
                                    policy == NativeTransportPolicy::PreserveStep,
                                )
                                .await?
                        } else {
                            fm.step_with_state_for(
                                &token,
                                crate::fm::FmStep::Previous,
                                attempt,
                                policy == NativeTransportPolicy::PreserveStep,
                            )
                            .await?
                        }
                    }
                    PlayerAction::Play => {
                        if let Some(operation) = fm_operation.as_ref() {
                            operation
                                .resume(
                                    &token,
                                    false,
                                    attempt,
                                    policy == NativeTransportPolicy::RestartStopped,
                                )
                                .await?
                        } else {
                            fm.resume_with_start_for(
                                &token,
                                false,
                                attempt,
                                policy == NativeTransportPolicy::RestartStopped,
                            )
                            .await?
                        }
                    }
                    PlayerAction::Restore => {
                        if let Some(operation) = fm_operation.as_ref() {
                            operation.resume(&token, true, attempt, false).await?
                        } else {
                            fm.resume_for(&token, true, attempt).await?
                        }
                    }
                    PlayerAction::Mode { mode } => {
                        fm.repeat_checked_for(
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
                            current.fence(),
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
            PlayerAction::PlayMissingDownloadOnline { .. } => {
                unreachable!("handled before user transport commands")
            }
            PlayerAction::NextPlay { .. }
            | PlayerAction::SearchNextPlay { .. }
            | PlayerAction::QueuePlaylist { .. }
            | PlayerAction::QueueVoice { .. }
            | PlayerAction::QueueAlbum { .. }
            | PlayerAction::QueueAlbumBatch { .. }
            | PlayerAction::QueueDailyRecommendations { .. }
            | PlayerAction::QueueStyleRecommendations { .. }
            | PlayerAction::QueueArtistSongs { .. }
            | PlayerAction::QueueDownloads { .. }
            | PlayerAction::QueueLocal { .. }
            | PlayerAction::DropLocalIntoQueue { .. }
            | PlayerAction::DropPlaylistSongIntoQueue { .. }
            | PlayerAction::DropPlaylistSongsIntoQueue { .. }
            | PlayerAction::QueuePlaylistBatch { .. }
            | PlayerAction::QueueCloudBatch { .. } => {
                unreachable!("handled before user transport commands")
            }
            PlayerAction::Equalizer {
                expected_revision,
                settings,
                preset,
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
                    preset,
                }
            }
            PlayerAction::SaveCustomEqualizer { expected_revision } => {
                let revision = expected_revision
                    .parse::<u64>()
                    .map_err(|_| Error::InvalidInput("invalid equalizer revision"))?;
                if revision.to_string() != expected_revision {
                    return Err(Error::InvalidInput("invalid equalizer revision"));
                }
                PlaybackCommand::SaveCustomEqualizer {
                    expected_revision: revision,
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
            PlayerAction::RepairQuality => PlaybackCommand::RepairQuality,
            PlayerAction::Play if policy == NativeTransportPolicy::RestartStopped => {
                PlaybackCommand::PlayFromStart
            }
            PlayerAction::Play => PlaybackCommand::Play,
            PlayerAction::Pause { paused } => PlaybackCommand::Pause(paused),
            PlayerAction::PauseFor { .. }
            | PlayerAction::SeekFor { .. }
            | PlayerAction::RefreshFor { .. } => {
                unreachable!("source control normalized before generic action")
            }
            PlayerAction::Seek { seconds } => PlaybackCommand::Seek(seconds),
            PlayerAction::Volume { value } => PlaybackCommand::Volume(value),
            PlayerAction::Speed { value } => PlaybackCommand::Speed(value),
            PlayerAction::SpeedFor { scope, value } => PlaybackCommand::SpeedFor { scope, value },
            PlayerAction::Next if policy == NativeTransportPolicy::PreserveStep => {
                PlaybackCommand::StepPreservingState(crate::playback::Step::Next)
            }
            PlayerAction::Previous if policy == NativeTransportPolicy::PreserveStep => {
                PlaybackCommand::StepPreservingState(crate::playback::Step::Previous)
            }
            PlayerAction::Next => PlaybackCommand::Next,
            PlayerAction::Previous => PlaybackCommand::Previous,
            PlayerAction::StepLoading { .. } => unreachable!("handled before generic action"),
            PlayerAction::Stop => PlaybackCommand::Stop,
            PlayerAction::Restore => PlaybackCommand::Restore,
            PlayerAction::Refresh => PlaybackCommand::RefreshSource,
            PlayerAction::Quality { level } | PlayerAction::QualityFor { level, .. } => {
                PlaybackCommand::Quality(level)
            }
            PlayerAction::Mode { mode } => PlaybackCommand::Mode(mode),
            PlayerAction::ModeFor { .. } => {
                unreachable!("fenced mode handled before generic action")
            }
            PlayerAction::QueueFor { .. } => {
                unreachable!("displayed queue edit normalized before generic action")
            }
            PlayerAction::Select { entry_id } => PlaybackCommand::Select(entry_id),
            PlayerAction::Remove { entry_ids } => {
                self.removal_command(&token, &current, queue_guard.is_some(), entry_ids)?
            }
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
        } else if let (true, Some(expected), Some(lease), Some(intent)) =
            (source_resume, source_guard.as_ref(), lease, control_intent)
        {
            let focus = self.focus.clone();
            let selection = self.player.selection.clone();
            controller
                .command_guarded(
                    &token,
                    command,
                    expected.clone(),
                    Cancellation::new(move || {
                        selection.load(std::sync::atomic::Ordering::Acquire) == intent
                            && focus.check(lease, super::focus::Owner::Audio).is_ok()
                    }),
                )
                .await?
        } else if let Some(expected) = quality_guard.or(queue_guard).or(source_guard) {
            controller
                .command_guarded(&token, command, expected, Cancellation::default())
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
            clear_context: self.queue_clear_context(&token)?,
            epoch: epoch.into(),
            revision: snapshot.queue_revision.to_string(),
            current: queue
                .current
                .as_ref()
                .filter(|id| {
                    queue
                        .items
                        .iter()
                        .any(|item| &item.entry_id == *id && item.visible_in_queue())
                })
                .cloned(),
            tracks: queue
                .items
                .iter()
                .filter(|item| item.visible_in_queue())
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
