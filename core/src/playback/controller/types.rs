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
}
#[derive(Clone, Debug, Default)]
pub struct PrefetchSnapshot {
    pub state: &'static str,
    pub entry_id: Option<String>,
}
/// Native snapshot; a host must map selected resources to its public DTO.
#[derive(Clone)]
pub struct PlaybackSnapshot {
    /// Native startup policy state, excluded from the public player DTO.
    pub startup_pending: bool,
    pub startup_autoplay: bool,
    pub fm: bool,
    pub equalizer: crate::effects::equalizer::EqualizerSnapshot,
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
    pub selected: Option<QueueEntry>,
    pub position: Option<f64>,
    pub timeline: Option<TimelineSnapshot>,
    pub requested_quality: Option<String>,
    pub actual_quality: Option<String>,
    pub volume: f64,
    pub speed: f64,
    pub mode: PlaybackMode,
    pub queue_len: usize,
    pub error: Option<String>,
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
}
impl PlaybackSnapshot {
    pub fn fence(&self) -> PlaybackFence {
        PlaybackFence {
            control_revision: self.control_revision,
            queue_revision: self.queue_revision,
            play_id: self.play_id.clone(),
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
/// One native initialization per account binding. The first binding alone may
/// autoplay; the host must also hold audio media ownership before permitting it.
pub struct StartupRestore {
    pub expected: PlaybackFence,
    pub queue: Option<QueueSnapshot>,
    pub settings: crate::playback::preferences::StartupSettings,
    pub allow_autoplay: bool,
}
pub enum PlaybackCommand {
    Startup(StartupRestore),
    /// Consume a failed startup preparation without changing newer transport.
    SkipStartup(PlaybackFence),
    Equalizer {
        expected_revision: u64,
        settings: crate::effects::equalizer::EqualizerSettings,
    },
    RetryEqualizer,
    RepairEqualizer,
    Loudness {
        expected_revision: String,
        enabled: bool,
    },
    RetryLoudness,
    RepairLoudness,
    UpdateQueue(QueueUpdate),
    ApplyRemote(RemotePlayback),
    Replace {
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
    Reorder(Vec<String>),
    Mode(PlaybackMode),
    Play,
    /// Resume paused transport; a stopped selection starts at authorized zero.
    PlayFromStart,
    Pause(bool),
    Seek(f64),
    Volume(f64),
    Speed(f64),
    Next,
    Previous,
    /// System transports keep a paused or stopped player from starting sound.
    StepPreservingState(crate::playback::Step),
    Stop,
    /// Native media handoff: cancel pending loads or pause a loaded song.
    Suspend,
    Restore,
    RefreshSource,
    Quality(Quality),
    SyncQueue,
    Snapshot,
    Queue,
}
pub(super) struct Reply {
    pub state: PlaybackSnapshot,
    pub queue: Option<QueueSnapshot>,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) struct Preferences {
    pub volume: f64,
    pub speed: f64,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            volume: 100.0,
            speed: 1.0,
        }
    }
}
