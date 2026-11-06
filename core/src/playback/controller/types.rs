use crate::{
    account::SessionToken,
    membership::Quality,
    playback::{PlaybackMode, QueueEntry, QueueSnapshot, TimelineSnapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    #[default]
    Idle,
    Preparing,
    Loading,
    Playing,
    Paused,
    Buffering,
    Ended,
    Error,
}
impl PlaybackStatus {
    pub(super) fn active(self) -> bool {
        matches!(self, Self::Playing | Self::Paused | Self::Buffering)
    }

    pub(crate) fn permits_minibar_step(self) -> bool {
        self.active() || self == Self::Ended
    }
}
#[derive(Clone, Debug, Default)]
pub struct PrefetchSnapshot {
    pub state: &'static str,
    pub entry_id: Option<String>,
}
/// Decoder-backed Scene FM identity. Only the playback actor mints this;
/// a provisional queue selection or persisted source alone is insufficient.
#[derive(Clone)]
pub(super) struct CommittedSceneVoice {
    scene_id: String,
    voice_id: String,
    scene_revision: u64,
}
impl CommittedSceneVoice {
    pub(super) fn from_entry(entry: &QueueEntry) -> Option<Self> {
        if entry.source["kind"] != "scene_fm" {
            return None;
        }
        Some(Self {
            scene_id: entry.source["scene_id"].as_str()?.into(),
            voice_id: entry.resource["voice_id"].as_str()?.into(),
            scene_revision: entry.source["scene_revision"].as_u64()?,
        })
    }
}
/// Native snapshot; a host must map selected resources to its public DTO.
#[derive(Clone)]
pub struct PlaybackSnapshot {
    /// Native startup policy state, excluded from the public player DTO.
    pub startup_pending: bool,
    pub startup_autoplay: bool,
    pub queue_more_available: bool,
    pub queue_more_tip_eligible: bool,
    pub fm: bool,
    pub intelligence: Option<crate::playback::intelligence::IntelligenceContext>,
    pub intelligence_operation: Option<String>,
    pub equalizer: crate::effects::equalizer::EqualizerSnapshot,
    pub bound_parametric: Option<super::parametric::ParametricBindingStatus>,
    /// PCM observed for the configured component on the current playing entry.
    pub parametric_processing: bool,
    pub device_parametric: Option<super::parametric::DeviceParametricView>,
    pub loudness: crate::effects::loudness::LoudnessSnapshot,
    pub revision: u64,
    /// Accepted native control intent; engine clock polls do not advance it.
    pub control_revision: u64,
    /// Last applied seek; volume/pause/clock updates do not change this.
    pub seek_revision: u64,
    pub queue_revision: u64,
    pub account: SessionToken,
    pub state: PlaybackStatus,
    /// Includes a pause accepted while the source/decoder is still loading.
    pub paused: bool,
    pub play_id: Option<String>,
    pub(crate) ended_minibar_receipt: Option<uuid::Uuid>,
    pub(super) committed_scene_voice: Option<CommittedSceneVoice>,
    pub selected: Option<QueueEntry>,
    pub position: Option<f64>,
    pub buffered_through: Option<f64>,
    /// The current native decoder is still seeking; not a transport fence.
    pub native_seeking: bool,
    pub timeline: Option<TimelineSnapshot>,
    pub quality_preferences: crate::playback::quality_preferences::QualityPreferencesSnapshot,
    pub can_change_quality: bool,
    /// A catalog search row is currently decoded from its verified publication.
    pub catalog_file_source: bool,
    pub quality_preferences_apply: bool,
    pub requested_quality: Option<String>,
    pub actual_quality: Option<String>,
    pub volume: f64,
    pub unmute_volume: f64,
    pub speed: f64,
    pub speed_scope: PlaybackSpeedScope,
    pub(crate) song_speed: f64,
    pub(crate) song_speed_view: Option<super::SongSpeedView>,
    pub mode: PlaybackMode,
    pub queue_len: usize,
    /// Public list count excludes a retained current decoder outside the list.
    pub visible_queue_len: usize,
    pub error: Option<crate::playback::failure::PlaybackFailure>,
    /// Local persistence diagnostic; must not be presented as an audio failure.
    pub history_error: Option<String>,
    pub prefetch: PrefetchSnapshot,
    pub position_reset: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaybackFence {
    pub control_revision: u64,
    pub queue_revision: u64,
    pub play_id: Option<String>,
    ended_minibar_receipt: Option<uuid::Uuid>,
}
impl PlaybackSnapshot {
    pub(crate) fn committed_scene_voice(&self, token: &SessionToken) -> Option<(&str, &str, u64)> {
        let voice = self.committed_scene_voice.as_ref()?;
        (&self.account == token).then_some((
            voice.scene_id.as_str(),
            voice.voice_id.as_str(),
            voice.scene_revision,
        ))
    }

    pub(crate) fn headphone_restore_ready(&self) -> bool {
        if self.state == PlaybackStatus::Idle {
            // The UI intentionally skips startup restore on an empty queue.
            // There is no pending selection that can load on its own.
            !self.startup_pending || (self.queue_len == 0 && self.selected.is_none())
        } else {
            !self.startup_pending && self.state.active()
        }
    }

    pub fn fence(&self) -> PlaybackFence {
        PlaybackFence {
            control_revision: self.control_revision,
            queue_revision: self.queue_revision,
            play_id: self.play_id.clone(),
            ended_minibar_receipt: self.ended_minibar_receipt,
        }
    }
}
/// Native-only atomic application of an already authorized remote command.
/// The account token and fence prevent older room work replacing newer input.
pub struct RemotePlayback {
    pub expected: PlaybackFence,
    pub entry_id: String,
    pub position: f64,
    pub paused: bool,
}
pub struct QueueTransport {
    pub position: Option<f64>,
    pub paused: bool,
}
/// Install an authoritative native queue and its selected transport together.
pub struct QueueUpdate {
    pub expected: PlaybackFence,
    pub queue: QueueSnapshot,
    pub transport: Option<QueueTransport>,
}
/// Native FM exit receipt. Both values are read together; the queue writer
/// compares them with the current SQLite rows before consuming the backup.
pub struct FmRestore {
    pub expected: PlaybackFence,
    pub backup_queue: Option<Value>,
    pub backup_position: Option<Value>,
}
/// Native callers must hold media ownership before an action that can start a
/// seed or select the first restored track. Recommendation completion does not
/// take ownership from later pause/stop/selection intents.
pub struct IntelligenceUpdate {
    pub expected: PlaybackFence,
    pub action: crate::playback::intelligence::IntelligenceAction,
}
/// One native initialization per account binding. The first binding alone may
/// autoplay; the host must also hold audio media ownership before permitting it.
pub struct StartupRestore {
    pub expected: PlaybackFence,
    pub queue: Option<QueueSnapshot>,
    pub settings: crate::playback::preferences::StartupSettings,
    pub allow_autoplay: bool,
}
pub enum PlaybackCommand {
    QueueMoreOpened {
        expected_revision: u64,
    },
    QueueMoreToggle {
        expected_revision: u64,
        enabled: bool,
    },
    Startup(StartupRestore),
    /// Consume a failed startup preparation without changing newer transport.
    SkipStartup(PlaybackFence),
    Equalizer {
        expected_revision: u64,
        settings: crate::effects::equalizer::EqualizerSettings,
        preset: Option<crate::effects::equalizer::EqualizerPreset>,
    },
    SaveCustomEqualizer {
        expected_revision: u64,
    },
    RetryEqualizer,
    RepairEqualizer,
    Loudness {
        expected_revision: String,
        enabled: bool,
    },
    RetryLoudness,
    RepairLoudness,
    RepairQuality,
    UpdateQueue(QueueUpdate),
    /// Explicit ordinary search row selection. Only this load may prefer an
    /// already authorized download; the queue keeps the catalog resource.
    PlaySearchCatalog(Option<QueueUpdate>),
    /// Install an ordinary queue with the exact same native selected entry,
    /// then keep playing or resume it without seeking or reloading.
    RetainCurrentQueueAndPlay(QueueSnapshot),
    /// Zone FM's explicit same-track case may retain its already authorized
    /// decoder; an incompatible source takes the normal load path.
    UpdateZoneFm {
        update: QueueUpdate,
        retain_paused: bool,
    },
    /// Explicit play-all with the recovered /newSongPublish first-track rule.
    ReplaceFollowLatest {
        resources: Vec<Value>,
        first_download: Option<std::sync::Arc<crate::playback::source::ResolvedSource>>,
    },
    ReplaceProfileRankPlaylist {
        resources: Vec<Value>,
        source: Value,
    },
    RestoreFm(FmRestore),
    Intelligence(IntelligenceUpdate),
    ApplyRemote(RemotePlayback),
    Replace {
        resources: Vec<Value>,
        index: usize,
        source: Value,
    },
    /// M404's explicit recommended-radio play-all, with first-load position.
    ReplacePodcastRecommendation {
        resources: Vec<Value>,
        index: usize,
        source: Value,
        position: f64,
        sequential: bool,
    },
    /// Replace from a fenced book row using its supplied checkpoint once.
    ReplaceListenedBook {
        resources: Vec<Value>,
        index: usize,
        source: Value,
    },
    Append {
        resources: Vec<Value>,
        source: Value,
    },
    Select(String),
    Remove(Vec<String>),
    /// Explicit displayed ordinary-queue removal; a paused successor may
    /// become audible only while the captured audio owner is still current.
    RemoveAndResume {
        entry_ids: Vec<String>,
        audio_owner: super::super::engine::Cancellation,
    },
    Reorder(Vec<String>),
    /// Fenced move from displayed rows; only this carries marker demotion.
    MoveDisplayed(super::super::QueueMovement),
    Mode(PlaybackMode),
    Play,
    /// Resume paused transport; a stopped selection starts at authorized zero.
    PlayFromStart,
    Pause(bool),
    Seek(f64),
    /// Fenced seek while preparing the already authorized ordinary source.
    SeekDuringLoad(f64),
    Volume(f64),
    Speed(f64),
    SpeedFor {
        scope: PlaybackSpeedScope,
        value: f64,
    },
    SongSpeed {
        expected_revision: u64,
        expected_entry: String,
        selection: super::SongSpeedSelection,
    },
    Next,
    Previous,
    /// System transports keep a paused or stopped player from starting sound.
    StepPreservingState(crate::playback::Step),
    Stop,
    /// Native media handoff: cancel pending loads or pause a loaded song.
    Suspend,
    Restore,
    RefreshSource,
    /// Explicit authenticated online authorization for the failed download
    /// entry. The actor accepts this only with a private client guard.
    UseMissingDownloadOnline {
        resource: Value,
        source: std::sync::Arc<super::super::source::ResolvedSource>,
        preference_revision: String,
    },
    Quality(Quality),
    /// Native trial provenance; the actor derives the fallback from its own
    /// preference and verifies the durable active journal at commit time.
    TrialQuality {
        right: crate::playback::quality::QualityRight,
        operation: String,
    },
    SyncQueue,
    Snapshot,
    Queue,
}
pub(super) struct Reply {
    pub state: PlaybackSnapshot,
    pub queue: Option<QueueSnapshot>,
    pub queue_more: Option<crate::playback::queue_more::QueueMoreView>,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) struct Preferences {
    pub volume: f64,
    #[serde(default = "default_unmute_volume")]
    pub unmute_volume: f64,
    pub speed: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song_speed: Option<super::song_speed::Saved>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackSpeedScope {
    #[default]
    Song,
    Voice,
}
impl PlaybackSpeedScope {
    pub(super) fn for_entry(entry: Option<&QueueEntry>) -> Self {
        if entry.is_some_and(|item| {
            !item.resource["voice_id"].is_null() || !item.resource["song"]["voice_id"].is_null()
        }) {
            Self::Voice
        } else {
            Self::Song
        }
    }
}
impl Preferences {
    pub(super) fn speed(self, scope: PlaybackSpeedScope) -> f64 {
        match scope {
            PlaybackSpeedScope::Song => self.speed,
            PlaybackSpeedScope::Voice => self.voice_speed.unwrap_or(self.speed),
        }
    }

    pub(super) fn set_speed(&mut self, scope: PlaybackSpeedScope, value: f64) {
        match scope {
            PlaybackSpeedScope::Song => {
                // A legacy shared value was visible in both modes. Preserve
                // it for voice before the first independent song adjustment.
                self.voice_speed.get_or_insert(self.speed);
                self.speed = value;
            }
            PlaybackSpeedScope::Voice => self.voice_speed = Some(value),
        }
    }
}
fn default_unmute_volume() -> f64 {
    10.0
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            volume: 100.0,
            unmute_volume: default_unmute_volume(),
            speed: 1.0,
            voice_speed: None,
            song_speed: None,
        }
    }
}
