mod copyright_notification;
mod volume_diagnosis;
pub use volume_diagnosis::{OutputDiagnosticRead, VolumeDiagnosisDismissal};
mod chorus;
mod comment_count;
mod dto;
mod history_batch;
mod history_radio_menu;
mod identity;
mod intelligence;
mod local_batch;
mod mixed_playlist;
pub use lyrics::LyricBountyIntent;
mod lyrics;
mod minibar_skip;
mod ordinary_skip_authority;
pub(super) use ordinary_skip_authority::OrdinarySkipHost;
mod missing_download;
mod mode;
mod next_play;
pub(in crate::client) mod origin;
mod output;
pub(super) mod playlist_batch;
mod playlist_trial;
mod podcast_browse;
mod podcast_purchased;
mod podcast_recommended;
mod power;
mod preferences;
mod prepared_selection;
mod quality;
mod queue_catalog_reveal;
pub use queue_catalog_reveal::QueueCatalogFileReveal;
mod playlist_duration;
mod queue_click;
mod queue_collection;
mod queue_download_menu;
mod queue_offline_playlist;
pub use queue_download_menu::QueueDownloadFileReveal;
mod queue_drop;
mod queue_edit;
mod queue_like;
mod queue_local_menu;
pub use queue_local_menu::QueueLocalFileReveal;
mod queue_menu;
mod queue_more_playlist;
pub(super) mod queue_playlist;
mod queue_voice_menu;
pub use queue_voice_menu::QueueVoiceFileReveal;
mod reports;
mod search_artist;
mod search_podcast;
mod search_voice_menu;
mod selection;
mod song_share;
mod source_control;
mod startup;
mod vinyl_comments;
mod vinyl_composer;
mod vinyl_likes;
mod vinyl_related_playlists;
mod vinyl_report;
pub use vinyl_related_playlists::{VinylRelatedPlaylistView, VinylRelatedPlaylistsView};
mod vinyl_similar;
pub use vinyl_similar::{
    VinylRecommendationPlacement, VinylSimilarAction, VinylSimilarSongView, VinylSimilarSongsView,
};
mod podcast_resume;
mod vinyl_wiki;
mod voice_detail;
mod voice_recommendations;
mod voice_scope;
pub use podcast_resume::{PlayerPodcastResumeAction, PlayerPodcastResumeView};
pub use voice_recommendations::{
    VinylVoiceRecommendationCard, VinylVoiceRecommendationCount, VinylVoiceRecommendationsView,
};
mod voice_songs;
pub use voice_detail::{
    PlayerVoiceCreator, PlayerVoiceDetail, PlayerVoicePlayCount, PlayerVoiceRadio,
    VinylVoiceDetailTarget, VinylVoiceDetailView,
};
pub use voice_songs::{
    PlayerVoiceSongsView, VoiceSongAction, VoiceSongCollectCreation, VoiceSongCollectPlan,
    VoiceSongCollectResult, VoiceSongDownloadCreation, VoiceSongDownloadDecision,
    VoiceSongDownloadPicker, VoiceSongDownloadPlan, VoiceSongGroupDownloadConfirmation,
    VoiceSongGroupDownloadDecision, VoiceSongGroupDownloadHint, VoiceSongGroupDownloadPlan,
    VoiceSongGroupPlaylistCreation, VoiceSongGroupPlaylistResult, VoiceSongLikedDestination,
    VoiceSongPurchasedDownloadCommand, VoiceSongPurchasedDownloadResult, VoiceSongRowAvailability,
    VoiceSongSelectionAction, VoiceSongSelectionKind,
};
mod zone_playlist_batch;
use super::{MusicClient, mapping};
pub use crate::playback::controller::QueueMoreTipAction;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::LocalLibrary,
    lyrics::LyricsService,
    playback::{
        controller::{
            PlaybackCommand, PlaybackController, PlaybackHostOptions, PlaybackSnapshot,
            PlaybackSpeedScope, PlaybackStatus,
        },
        engine::{AudioEngine, Cancellation, MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    storage::StateStore,
};
pub use dto::*;
pub use intelligence::{IntelligenceCommand, IntelligenceEligibility, IntelligenceOrigin};
pub use output::{OutputCommand, OutputEngineView, OutputView};
pub use power::PlaybackPowerView;
pub use preferences::{StartupPreferencesCommand, StartupPreferencesView};
pub(super) use quality::config::music_package_hires;
pub use quality::{
    QualityChoiceView, QualityDetailsView, QualityMenuPresentation, QualityTrialCommand,
    QualityTrialOutcome, QualityTrialView,
};
pub use queue_click::{QueueClickPreferencesCommand, QueueClickPreferencesView};
pub use reports::LyricReportView;
pub use song_share::PlayerSongShareTarget;
pub(super) use song_share::mark_metadata as mark_current_song_share_metadata;
use std::sync::{Arc, Mutex, atomic::AtomicU64};
use tokio::sync::{OnceCell, watch};
pub use vinyl_comments::VinylCommentPageView;
pub use vinyl_likes::VinylCommentLikeAction;
pub use vinyl_wiki::{VinylWikiCardView, VinylWikiMetaView, VinylWikiView};

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
    queue_more_enabled: bool,
    volume_diagnosis: volume_diagnosis::Host,
    power: Option<Arc<crate::playback::keep_awake::PowerControl>>,
    ordinary_skip: OrdinarySkipHost,
    song_speed: watch::Receiver<crate::playback::controller::SongSpeedObservation>,
    power_assignment: tokio::sync::Mutex<Option<power::Assignment>>,
    queue_more_memberships: queue_more_playlist::MembershipCache,
    queue_catalog_reveal: queue_catalog_reveal::Host,
    song_share: song_share::Host,
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    pub(super) factory: EngineFactory,
    output: Arc<crate::playback::output::OutputRuntime>,
    inspector: OnceCell<Arc<Mutex<Box<dyn AudioEngine>>>>,
    pub(super) controller: OnceCell<Arc<PlaybackController>>,
    fm_service: OnceCell<Arc<crate::fm::FmService>>,
    fm: OnceCell<Arc<crate::fm::player::FmPlayer>>,
    pub(super) selection: Arc<AtomicU64>,
    prepared_selection: Mutex<Option<prepared_selection::PreparedSelection>>,
    prepared_generation: Arc<AtomicU64>,
    focus: Arc<super::focus::MediaFocus>,
    lyrics: Mutex<Option<lyrics::ActiveLyrics>>,
    chorus: Arc<chorus::Host>,
    room_detachment: super::together::RoomDetachment,
    vinyl_draft: Mutex<Option<vinyl_composer::VinylDraft>>,
    streaming_cache: Option<Arc<crate::playback::stream_cache::StreamingCache>>,
    vinyl_similar: Arc<vinyl_similar::Host>,
    voice_detail_read: voice_detail::Host,
    voice_recommendations: voice_recommendations::Host,
    voice_songs: Arc<voice_songs::Host>,
}
impl PlayerHost {
    pub fn new(
        session: Arc<AccountSession>,
        store: Arc<StateStore>,
        focus: Arc<super::focus::MediaFocus>,
        output: Arc<crate::playback::output::OutputRuntime>,
        ordinary_skip: OrdinarySkipHost,
        song_speed: watch::Receiver<crate::playback::controller::SongSpeedObservation>,
        room_detachment: super::together::RoomDetachment,
    ) -> Self {
        Self {
            intelligence: Arc::new(intelligence::IntelligenceHost::default()),
            relay_enabled: false,
            queue_more_enabled: false,
            volume_diagnosis: Default::default(),
            power: None,
            ordinary_skip,
            song_speed,
            power_assignment: Default::default(),
            queue_more_memberships: Default::default(),
            queue_catalog_reveal: Default::default(),
            song_share: Default::default(),
            session,
            store,
            factory: Arc::new(|| Ok(Box::new(MpvEngine::open(MpvConfig::default())?))),
            controller: OnceCell::new(),
            fm_service: OnceCell::new(),
            fm: OnceCell::new(),
            selection: Arc::new(AtomicU64::new(0)),
            prepared_selection: Mutex::new(None),
            prepared_generation: Arc::new(AtomicU64::new(0)),
            lyrics: Mutex::new(None),
            chorus: Default::default(),
            room_detachment,
            vinyl_draft: Mutex::new(None),
            streaming_cache: None,
            vinyl_similar: Default::default(),
            voice_detail_read: Default::default(),
            voice_recommendations: Default::default(),
            voice_songs: Default::default(),
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
                let mut resolver = SourceResolver::new(self.session.clone(), Some(library));
                resolver.streaming_cache = self.streaming_cache.clone();
                let resolver = Arc::new(resolver);
                let controller = Arc::new(PlaybackController::start_inner(
                    self.session.clone(),
                    self.store.clone(),
                    resolver,
                    Box::new(lease.wrap(Box::new(super::focus::Focused {
                        inner: engine,
                        focus: self.focus.clone(),
                        owner: super::focus::Owner::Audio,
                    }))),
                    PlaybackHostOptions {
                        relay_enabled: self.relay_enabled,
                        queue_more_enabled: self.queue_more_enabled,
                        power: self.power.clone(),
                        song_speed: Some(self.song_speed.clone()),
                        chorus_triggers: Some(
                            self.chorus
                                .triggers(self.session.clone(), self.room_detachment.clone()),
                        ),
                        ordinary_eof: Some(
                            self.ordinary_skip
                                .eof(self.focus.clone(), self.selection.clone()),
                        ),
                    },
                )?);
                self.chorus.bind(&controller)?;
                Ok(controller)
            })
            .await
    }
    pub(super) async fn fm(&self) -> Result<&Arc<crate::fm::player::FmPlayer>> {
        self.fm
            .get_or_try_init(|| async {
                Ok(Arc::new(crate::fm::player::FmPlayer::with_service(
                    self.session.clone(),
                    self.fm_service().await?.clone(),
                    self.controller().await?.clone(),
                    self.selection.clone(),
                )?))
            })
            .await
    }

    /// Initialise only the catalogue/queue service, independently of the
    /// output device, playback controller or FM worker.
    pub(super) async fn fm_service(&self) -> Result<&Arc<crate::fm::FmService>> {
        self.fm_service
            .get_or_try_init(|| async {
                Ok(Arc::new(crate::fm::FmService::new(self.session.clone())?))
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
    let selected = value.selected.as_ref();
    let track = selected.map(|item| track(&item.resource)).transpose()?;
    let title_alias = selected.zip(track.as_ref()).and_then(|(item, track)| {
        let resource = &item.resource;
        (value.speed_scope == PlaybackSpeedScope::Song
            && identity::kind(resource) != PlayerResourceKind::Voice
            && resource["download_voice_id"].is_null()
            && track.voice_id.is_none()
            && crate::protocol::api::identifier(&resource["id"])
                .ok()
                .as_deref()
                == Some(track.id.as_str()))
        .then(|| track.title_names.display())
        .flatten()
    });
    Ok(PlayerView {
        current_song_share_available: song_share::available(value),
        audiobook_skip_available: token.uid.is_some()
            && super::podcast::audiobook::active_radio(value).is_some(),
        fm: value.fm,
        zone_fm_rule_id: value.selected.as_ref().and_then(|entry| {
            (value.fm && entry.source["kind"] == "fm" && entry.source["source"]["kind"] == "zone")
                .then(|| crate::protocol::api::identifier(&entry.source["source"]["rule_id"]).ok())
                .flatten()
        }),
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
            spatial_status: value.equalizer.spatial_status,
            damaged: value.equalizer.damaged,
            source_bypassed: value.equalizer.source_bypassed,
        },
        bound_parametric: value.bound_parametric,
        applied_effect_id: value.applied_effect_id,
        parametric_processing: value.parametric_processing,
        device_parametric: value.device_parametric.clone(),
        epoch: token.generation.to_string(),
        revision: value.revision.to_string(),
        queue_revision: value.queue_revision.to_string(),
        queue_more_available: value.queue_more_available,
        queue_more_tip_eligible: value.queue_more_tip_eligible,
        state: value.state,
        pending_paused: matches!(
            value.state,
            PlaybackStatus::Preparing | PlaybackStatus::Loading
        )
        .then_some(value.paused),
        play_id: value.play_id.clone(),
        ended_minibar_receipt: value
            .ended_minibar_receipt
            .map(|id| id.simple().to_string()),
        entry_id: value.selected.as_ref().map(|i| i.entry_id.clone()),
        download_task_id: value
            .selected
            .as_ref()
            .and_then(|i| i.resource["download_task_id"].as_str())
            .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(str::to_owned),
        resource_kind: value.selected.as_ref().map(|i| identity::kind(&i.resource)),
        vinyl_similar_origin: value.selected.as_ref().is_some_and(|entry| {
            identity::kind(&entry.resource) == PlayerResourceKind::Catalog
                && entry.source["kind"] == "vinyl_similar"
        }),
        search_origin: !value.fm && value.selected.as_ref().is_some_and(origin::search),
        track,
        title_alias,
        voice_detail: voice_detail::project(value),
        origin: value
            .selected
            .as_ref()
            .and_then(|i| origin::project(&i.source)),
        origin_name: value
            .selected
            .as_ref()
            .filter(|_| !value.fm)
            .and_then(origin::display_name)
            .map(str::to_owned),
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
        catalog_file_source: value.catalog_file_source,
        requested_quality: value.requested_quality.clone(),
        actual_quality: value.actual_quality.clone(),
        volume: value.volume,
        unmute_volume: value.unmute_volume,
        speed: value.speed,
        speed_scope: value.speed_scope,
        song_speed: value.song_speed_view.clone(),
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
    /// The window host must check foreground before calling Begin.
    pub async fn queue_more_tip(
        &self,
        epoch: &str,
        action: QueueMoreTipAction,
    ) -> Result<Option<String>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let result = self
            .player
            .controller()
            .await?
            .queue_more_tip(&token, action)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
    }

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
    /// Native host configuration, not a renderer supplied capability.
    pub fn with_queue_recommendations(mut self) -> Self {
        self.player.queue_more_enabled = true;
        self
    }
    /// Native opt-in before constructing the audio controller. The desktop
    /// does not opt in by default and the renderer cannot select this path.
    pub fn with_streaming_media_cache(mut self, capacity_bytes: u64) -> Result<Self> {
        if self.player.controller.get().is_some() {
            return Err(Error::State(
                "streaming media cache needs an unstarted player",
            ));
        }
        self.player.streaming_cache = Some(crate::playback::stream_cache::StreamingCache::open(
            &self.player.store,
            capacity_bytes,
        )?);
        Ok(self)
    }

    /// Enable the device-scoped cache settings before playback starts.
    /// A failed setup leaves the existing stock host and its player intact.
    pub fn configure_streaming_media_cache(&mut self) -> Result<()> {
        if self.player.controller.get().is_some() {
            return Err(Error::State(
                "streaming media cache needs an unstarted player",
            ));
        }
        let cache =
            crate::playback::stream_cache::StreamingCache::configured(self.player.store.clone())?;
        self.player.streaming_cache = Some(cache);
        Ok(())
    }

    pub fn streaming_media_cache_status(
        &self,
    ) -> Result<Option<crate::playback::StreamingMediaCacheStatus>> {
        match &self.player.streaming_cache {
            Some(cache) => cache.status(),
            None => Ok(None),
        }
    }

    pub fn set_streaming_media_cache_capacity(
        &self,
        expected_revision: &str,
        capacity_bytes: u64,
    ) -> Result<crate::playback::StreamingMediaCacheStatus> {
        self.playback_cache_settings()?
            .set_capacity(expected_revision, capacity_bytes)
    }

    pub fn clear_streaming_media_cache_checked(
        &self,
        expected_revision: &str,
    ) -> Result<crate::playback::StreamingMediaCacheStatus> {
        self.playback_cache_settings()?
            .clear_checked(expected_revision)
    }

    fn playback_cache_settings(&self) -> Result<&crate::playback::stream_cache::StreamingCache> {
        self.player
            .streaming_cache
            .as_deref()
            .ok_or(Error::State("streaming cache preferences are unavailable"))
    }

    /// Retire only this native host's process-owned playback cache. An
    /// already-opened stream retains its handle until the decoder closes it.
    pub fn clear_streaming_media_cache(&self) -> Result<()> {
        self.playback_cache_settings()?.clear_native()
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
    /// Native main-window stream adapter only. This can observe accepted
    /// decisions and cannot authorize or publish one.
    pub fn subscribe_ordinary_skip(
        &self,
        epoch: &str,
    ) -> Result<crate::playback::controller::OrdinarySkipNotices> {
        let token = self.player_token(epoch)?;
        self.player
            .controller
            .get()
            .ok_or(Error::State("player stream is not initialized"))?
            .ordinary_skip_notices(token)
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
        if let PlayerAction::SongSpeed {
            expected_revision,
            expected_entry,
            selection,
        } = &action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "song speed requires the playback panel",
                ));
            }
            let token = self.player_token(epoch)?;
            let _room = self.together.gate.lock().await;
            if self.together_attached(&token, None)? {
                return Err(Error::State("together requires normal playback speed"));
            }
            // Validate the current native account/credential presentation; an
            // old renderer event must not activate a cached previous owner.
            self.pc_presentation()?;
            self.session.check_generation(&token)?;
            let expected_revision = expected_revision
                .parse()
                .map_err(|_| Error::InvalidInput("invalid song speed revision"))?;
            let value = self
                .player
                .controller()
                .await?
                .command(
                    &token,
                    PlaybackCommand::SongSpeed {
                        expected_revision,
                        expected_entry: expected_entry.clone(),
                        selection: *selection,
                    },
                )
                .await?;
            self.session.check_generation(&token)?;
            return project(&value, &token);
        }
        if matches!(action, PlayerAction::Startup) {
            return self.startup_player(epoch).await;
        }
        let recommendation = match &action {
            PlayerAction::QueueMoreOpened { expected_revision } => Some((expected_revision, None)),
            PlayerAction::QueueMoreToggle {
                expected_revision,
                enabled,
            } => Some((expected_revision, Some(*enabled))),
            _ => None,
        };
        if let Some((expected_revision, enabled)) = recommendation {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "recommendations require the queue panel",
                ));
            }
            let expected_revision = expected_revision
                .parse::<u64>()
                .map_err(|_| Error::InvalidInput("invalid queue revision"))?;
            let token = self.player_token(epoch)?;
            let _room = self.together.gate.lock().await;
            if self.together_attached(&token, None)? {
                return Err(Error::State("room owns the playback queue"));
            }
            let command = match enabled {
                Some(enabled) => PlaybackCommand::QueueMoreToggle {
                    expected_revision,
                    enabled,
                },
                None => PlaybackCommand::QueueMoreOpened { expected_revision },
            };
            let value = self
                .player
                .controller()
                .await?
                .command(&token, command)
                .await?;
            self.session.check_generation(&token)?;
            return project(&value, &token);
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
        if let PlayerAction::StepEndedMiniBar {
            receipt,
            entry_id,
            expected_queue_revision,
            direction,
        } = action
        {
            if native.is_some() || policy != NativeTransportPolicy::Ordinary {
                return Err(Error::InvalidInput(
                    "bottom bar control used as native media command",
                ));
            }
            return self
                .step_ended_minibar(
                    epoch,
                    &receipt,
                    &entry_id,
                    &expected_queue_revision,
                    direction,
                )
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
        if let PlayerAction::SearchLyricGroupPlayback {
            operation,
            anchor,
            rows,
            quality,
            expected_play_id,
            expected_queue_revision,
        } = action
        {
            if native.is_some() || policy != NativeTransportPolicy::Ordinary {
                return Err(Error::InvalidInput(
                    "lyric search group used as native media command",
                ));
            }
            return self
                .search_group_playback(
                    epoch,
                    next_play::SearchGroupRequest {
                        action: match operation {
                            super::search::LyricGroupOperation::Play => {
                                next_play::SearchGroupAction::Play
                            }
                            super::search::LyricGroupOperation::Next => {
                                next_play::SearchGroupAction::Next
                            }
                        },
                        selection: next_play::SearchGroupRows::Lyrics { anchor, rows },
                        quality,
                        expected_play_id,
                        expected_queue_revision,
                    },
                )
                .await;
        }
        let search_group_play = matches!(action, PlayerAction::SearchGroupPlay { .. });
        if let PlayerAction::SearchGroupNextPlay {
            anchor,
            rows,
            quality,
            expected_play_id,
            expected_queue_revision,
        }
        | PlayerAction::SearchGroupPlay {
            anchor,
            rows,
            quality,
            expected_play_id,
            expected_queue_revision,
        } = action
        {
            if native.is_some() || policy != NativeTransportPolicy::Ordinary {
                return Err(Error::InvalidInput(
                    "search group used as native media command",
                ));
            }
            return self
                .search_group_playback(
                    epoch,
                    next_play::SearchGroupRequest {
                        action: if search_group_play {
                            next_play::SearchGroupAction::Play
                        } else {
                            next_play::SearchGroupAction::Next
                        },
                        selection: next_play::SearchGroupRows::Songs { anchor, rows },
                        quality,
                        expected_play_id,
                        expected_queue_revision,
                    },
                )
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
            PlayerAction::DropAlbumSongsIntoQueue {
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
                        DropSource::Album {
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
            PlayerAction::DropMixedPlaylistSongsIntoQueue {
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
                        DropSource::MixedPlaylist {
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
            PlayerAction::DropSearchSongsIntoQueue {
                rows,
                quality,
                expected_queue_revision,
                expected_play_id,
                before_entry_id,
            } => {
                return self
                    .drop_into_queue(
                        epoch,
                        DropSource::Search { rows, quality },
                        DropTarget {
                            expected_queue_revision,
                            expected_play_id,
                            before_entry_id,
                        },
                    )
                    .await;
            }
            PlayerAction::DropSearchLyricsIntoQueue {
                anchor,
                rows,
                quality,
                expected_queue_revision,
                expected_play_id,
                before_entry_id,
            } => {
                return self
                    .drop_into_queue(
                        epoch,
                        DropSource::Lyrics {
                            anchor,
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
        if let PlayerAction::QueueSortedPlaylist {
            scope,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_sorted_playlist(epoch, scope, quality, &expected_queue_revision)
                .await;
        }
        if let PlayerAction::QueueOfflinePlaylist {
            id,
            revision,
            cached_at_ms,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_offline_playlist(
                    epoch,
                    &id,
                    &revision,
                    cached_at_ms,
                    &expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueMixedPlaylist {
            id,
            revision,
            track,
            quality,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::MixedPlaylist {
                        id,
                        revision,
                        track,
                        quality,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueVoice {
            id,
            program,
            expected_queue_revision,
        } = action
        {
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Voice { id, program },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueRecentProgram {
            target,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "recent program queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::RecentVoice(target),
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueuePurchasedProgram {
            target,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "purchased program queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::PurchasedVoice(target),
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueLikedProgram {
            target,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "liked program queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::LikedVoice(target),
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueLikedPageProgram {
            target,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "paged liked program queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::LikedPageVoice(target),
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
                    queue_playlist::QueueCollection::ArtistSongs {
                        pages,
                        quality,
                        selected: None,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueArtistSongsBatch {
            pages,
            ids,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "artist batch queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::ArtistSongs {
                        pages,
                        quality,
                        selected: Some(ids),
                    },
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
        if let PlayerAction::QueueAlbumDetail {
            album,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "album detail queue used as native media command",
                ));
            }
            return self
                .queue_collection(
                    epoch,
                    queue_playlist::QueueCollection::Album {
                        id: album.id.clone(),
                        quality,
                        detail: Some(album),
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
                    queue_playlist::QueueCollection::Album {
                        id,
                        quality,
                        detail: None,
                    },
                    expected_queue_revision,
                )
                .await;
        }
        if let PlayerAction::QueueVerticalZonePlaylistAll {
            id,
            receipt,
            revision,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "zone playlist queue used as native media command",
                ));
            }
            return self
                .zone_playlist_queue_action(
                    epoch,
                    zone_playlist_batch::ZoneQueueSelection {
                        id,
                        receipt,
                        revision,
                        quality,
                        rows: zone_playlist_batch::ZoneQueueRows::Whole,
                    },
                    Some(expected_queue_revision),
                )
                .await;
        }
        if let PlayerAction::QueueVerticalZonePlaylistNext {
            id,
            receipt,
            revision,
            song_id,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "zone playlist queue used as native media command",
                ));
            }
            return self
                .zone_playlist_queue_action(
                    epoch,
                    zone_playlist_batch::ZoneQueueSelection {
                        id,
                        receipt,
                        revision,
                        quality,
                        rows: zone_playlist_batch::ZoneQueueRows::Next(song_id),
                    },
                    Some(expected_queue_revision),
                )
                .await;
        }
        if let PlayerAction::QueueVerticalZonePlaylistBatch {
            id,
            receipt,
            ids,
            revision,
            quality,
            expected_queue_revision,
        } = action
        {
            if native.is_some() {
                return Err(Error::InvalidInput(
                    "zone playlist batch used as native media command",
                ));
            }
            return self
                .zone_playlist_queue_action(
                    epoch,
                    zone_playlist_batch::ZoneQueueSelection {
                        id,
                        receipt,
                        revision,
                        quality,
                        rows: zone_playlist_batch::ZoneQueueRows::Batch(ids),
                    },
                    Some(expected_queue_revision),
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
        let mut minibar_step = None;
        let (action, source_target) = match action {
            PlayerAction::StepMiniBar {
                play_id,
                expected_queue_revision,
                direction,
            } => {
                if native.is_some() || policy != NativeTransportPolicy::Ordinary {
                    return Err(Error::InvalidInput(
                        "bottom bar control used as native media command",
                    ));
                }
                minibar_step = Some(direction);
                (
                    match direction {
                        crate::playback::Step::Next => PlayerAction::Next,
                        crate::playback::Step::Previous => PlayerAction::Previous,
                    },
                    Some((play_id, expected_queue_revision)),
                )
            }
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
        let (mut action, displayed_queue, clear_room, queue_movement) = match action {
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
                let (action, movement) = edit.into_action();
                (action, Some(expected_queue_revision), room, movement)
            }
            other => (other, None, None, None),
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
            if minibar_step.is_some() {
                source_control::validate_identity(&current, play_id, revision)?;
                if !current.state.permits_minibar_step() {
                    return Err(Error::StaleResponse);
                }
            } else {
                source_control::validate(
                    &current,
                    play_id,
                    revision,
                    matches!(action, PlayerAction::Refresh),
                )?;
            }
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
        let (queue_guard, queue_no_change) = if let Some(revision) = displayed_queue {
            let (fence, no_change) = self
                .displayed_queue_fence(&token, &revision, &mut action, queue_movement.as_ref())
                .await?;
            (Some(fence), no_change)
        } else {
            (None, false)
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
        if queue_no_change {
            let current = self
                .player
                .controller()
                .await?
                .command(&token, PlaybackCommand::Snapshot)
                .await?;
            if queue_guard != Some(current.fence()) {
                return Err(Error::QueueChanged);
            }
            self.session.check_generation(&token)?;
            return project(&current, &token);
        }
        let together_action = super::together::signal(&action);
        // Reject obsolete popup input before it can supersede another pending
        // selection. The actor fence below still covers the later race.
        if matches!(
            action,
            PlayerAction::Quality { .. } | PlayerAction::QualityFor { .. }
        ) {
            let controller = self.player.controller().await?;
            let current = controller
                .command(&token, PlaybackCommand::Snapshot)
                .await?;
            if let PlayerAction::QualityFor {
                play_id,
                expected_revision,
                ..
            } = &action
            {
                validate_quality_target(&current, play_id, expected_revision)?;
            }
            if current.catalog_file_source {
                return Err(Error::State("quality requires loaded online track"));
            }
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
        let lease = if source_resume || minibar_step.is_some() {
            // A verified bottom-bar step can keep the current entry. Reuse
            // its audio focus instead of transiently pausing it during a
            // handoff from Audio to itself.
            Some(self.audio_source_resume_lease(&token).await?)
        } else if matches!(
            action,
            PlayerAction::Play
                | PlayerAction::Pause { paused: false }
                | PlayerAction::Next
                | PlayerAction::Previous
                | PlayerAction::Select { .. }
        ) {
            Some(match native {
                Some(target)
                    if policy == NativeTransportPolicy::PreserveStep
                        && target.focus.owner == Some(super::focus::Owner::Audio) =>
                {
                    // A system step on the already-owned decoder keeps its
                    // exact lease. Revoking Audio here would let the native
                    // poll pause the old entry while a handoff waits.
                    self.session.check_generation(&token)?;
                    self.focus.check_snapshot(target.focus)?;
                    target.focus.generation
                }
                Some(target) => {
                    self.claim_native_media(&token, super::focus::Owner::Audio, target.focus)
                        .await?
                }
                None => self.claim_media(&token, super::focus::Owner::Audio).await?,
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
        let minibar_guard = if let (Some(direction), Some(lease), Some(intent)) =
            (minibar_step, lease, control_intent)
        {
            let focus = self.focus.clone();
            let selection = self.player.selection.clone();
            let guard = Cancellation::new(move || {
                selection.load(std::sync::atomic::Ordering::Acquire) == intent
                    && focus.check(lease, super::focus::Owner::Audio).is_ok()
            });
            if let Some(state) = self
                .try_minibar_skip(&token, controller, &current, direction, &guard)
                .await?
            {
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            Some(guard)
        } else {
            None
        };
        let command = match action {
            PlayerAction::QueueMoreOpened { .. } | PlayerAction::QueueMoreToggle { .. } => {
                unreachable!("handled before user transport commands")
            }
            PlayerAction::Startup => unreachable!("handled before user transport commands"),
            PlayerAction::PlayMissingDownloadOnline { .. } => {
                unreachable!("handled before user transport commands")
            }
            PlayerAction::NextPlay { .. }
            | PlayerAction::SearchNextPlay { .. }
            | PlayerAction::SearchGroupNextPlay { .. }
            | PlayerAction::SearchGroupPlay { .. }
            | PlayerAction::SearchLyricGroupPlayback { .. }
            | PlayerAction::QueuePlaylist { .. }
            | PlayerAction::QueueSortedPlaylist { .. }
            | PlayerAction::QueueOfflinePlaylist { .. }
            | PlayerAction::QueueMixedPlaylist { .. }
            | PlayerAction::QueueVoice { .. }
            | PlayerAction::QueueRecentProgram { .. }
            | PlayerAction::QueuePurchasedProgram { .. }
            | PlayerAction::QueueLikedProgram { .. }
            | PlayerAction::QueueLikedPageProgram { .. }
            | PlayerAction::QueueAlbum { .. }
            | PlayerAction::QueueAlbumDetail { .. }
            | PlayerAction::QueueAlbumBatch { .. }
            | PlayerAction::QueueDailyRecommendations { .. }
            | PlayerAction::QueueStyleRecommendations { .. }
            | PlayerAction::QueueArtistSongs { .. }
            | PlayerAction::QueueArtistSongsBatch { .. }
            | PlayerAction::QueueDownloads { .. }
            | PlayerAction::QueueLocal { .. }
            | PlayerAction::DropLocalIntoQueue { .. }
            | PlayerAction::DropPlaylistSongIntoQueue { .. }
            | PlayerAction::DropPlaylistSongsIntoQueue { .. }
            | PlayerAction::DropMixedPlaylistSongsIntoQueue { .. }
            | PlayerAction::DropSearchSongsIntoQueue { .. }
            | PlayerAction::DropSearchLyricsIntoQueue { .. }
            | PlayerAction::DropAlbumSongsIntoQueue { .. }
            | PlayerAction::QueuePlaylistBatch { .. }
            | PlayerAction::QueueVerticalZonePlaylistAll { .. }
            | PlayerAction::QueueVerticalZonePlaylistNext { .. }
            | PlayerAction::QueueVerticalZonePlaylistBatch { .. }
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
            PlayerAction::SongSpeed { .. } => unreachable!("song speed panel handled above"),
            PlayerAction::Next if policy == NativeTransportPolicy::PreserveStep => {
                PlaybackCommand::StepPreservingState(crate::playback::Step::Next)
            }
            PlayerAction::Previous if policy == NativeTransportPolicy::PreserveStep => {
                PlaybackCommand::StepPreservingState(crate::playback::Step::Previous)
            }
            PlayerAction::Next => PlaybackCommand::Next,
            PlayerAction::Previous => PlaybackCommand::Previous,
            PlayerAction::StepLoading { .. }
            | PlayerAction::StepMiniBar { .. }
            | PlayerAction::StepEndedMiniBar { .. } => {
                unreachable!("handled before generic action")
            }
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
            PlayerAction::Reorder { entry_ids } => match queue_movement {
                Some(movement) => PlaybackCommand::MoveDisplayed(movement),
                None => PlaybackCommand::Reorder(entry_ids),
            },
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
                .command_guarded(&token, command, expected, minibar_guard.unwrap_or_default())
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
        let (snapshot, queue, more_recommendations) = controller.queue_with_more(&token).await?;
        self.session.check_generation(&token)?;
        let more_visible = more_recommendations
            .as_ref()
            .is_some_and(|view| view.enabled);
        let clear_context = self.queue_clear_context(&token)?;
        let ordinary_queue = !snapshot.fm && clear_context.room_id.is_none();
        Ok(QueueView {
            fm: snapshot.fm,
            clear_context,
            epoch: epoch.into(),
            revision: snapshot.queue_revision.to_string(),
            more_recommendations,
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
                    let like_song_id = ordinary_queue
                        .then(|| queue_like::song_id(&i.resource))
                        .flatten();
                    let (collectible, catalog_menu) = if ordinary_queue {
                        queue_collection::row_actions(&i.resource)
                    } else {
                        (false, false)
                    };
                    let origin = ordinary_queue.then(|| origin::queue(i)).flatten();
                    let source_label = origin
                        .as_ref()
                        .and_then(|origin| origin::local_queue_source_name(i, origin))
                        .map(str::to_owned);
                    Ok(QueueTrack {
                        entry_id: i.entry_id.clone(),
                        track: track(&i.resource)?,
                        more_recommendation: more_visible
                            && crate::playback::queue_more::active(&queue, i),
                        origin,
                        source_label,
                        like_song_id,
                        catalog_menu,
                        voice_menu: ordinary_queue
                            && token.uid.is_some()
                            && queue.intelligence.is_none()
                            && queue_voice_menu::eligible(&i.resource),
                        local_menu: ordinary_queue
                            && queue.intelligence.is_none()
                            && queue_local_menu::eligible(i),
                        download_menu: (ordinary_queue
                            && token.uid.is_some()
                            && queue.intelligence.is_none())
                        .then(|| queue_download_menu::kind(i))
                        .flatten(),
                        collectible,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}
