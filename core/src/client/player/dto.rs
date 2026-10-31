use crate::{
    client::dto::TrackView,
    lyrics::{DisplayType, LyricLine, LyricPreferences, YrcInfo},
    membership::Quality,
    playback::{PlaybackMode, TimelineSnapshot, controller::PlaybackStatus},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub fm: bool,
    pub equalizer: EqualizerView,
    pub loudness: LoudnessView,
    pub epoch: String,
    pub revision: String,
    pub queue_revision: String,
    pub state: PlaybackStatus,
    pub play_id: Option<String>,
    pub entry_id: Option<String>,
    pub track: Option<TrackView>,
    pub origin: Option<PlayerOrigin>,
    pub position: Option<f64>,
    pub timeline: Option<TimelineSnapshot>,
    pub quality_preferences: QualityPreferencesView,
    pub can_change_quality: bool,
    pub requested_quality: Option<String>,
    pub actual_quality: Option<String>,
    pub volume: f64,
    pub unmute_volume: f64,
    pub speed: f64,
    pub mode: PlaybackMode,
    pub queue_len: usize,
    pub error: Option<&'static str>,
    pub history_warning: Option<&'static str>,
    pub position_reset: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityPreferencesView {
    pub applies_to_current: bool,
    pub revision: Option<String>,
    pub quality: Quality,
    pub damaged: bool,
}
/// Public resource identity only; queue source metadata never crosses IPC.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayerOrigin {
    Playlist { id: String },
    Album { id: String },
    Artist { id: String },
    Podcast { id: String },
}
#[derive(Clone, Serialize)]
pub struct EqualizerView {
    pub revision: String,
    pub settings: crate::effects::equalizer::EqualizerSettings,
    pub status: crate::effects::equalizer::EqualizerStatus,
    pub damaged: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessView {
    pub revision: Option<String>,
    pub enabled: bool,
    pub status: crate::effects::loudness::LoudnessStatus,
    pub gain_db: Option<f32>,
    pub damaged: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueTrack {
    pub entry_id: String,
    pub track: TrackView,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueView {
    pub fm: bool,
    pub epoch: String,
    pub revision: String,
    pub current: Option<String>,
    pub tracks: Vec<QueueTrack>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueCollectionView {
    pub epoch: String,
    pub revision: String,
    pub total: usize,
    pub unsupported: usize,
    pub duplicates: usize,
    pub tracks: Vec<super::super::dto::TrackView>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaySelection {
    Downloads {
        query: crate::client::dto::DownloadQuery,
        start_id: Option<String>,
    },
    History {
        history_kind: crate::history::HistoryKind,
        revision: String,
        ids: Vec<String>,
        index: usize,
        quality: Quality,
    },
    Catalog {
        pages: Vec<super::super::catalog::CatalogSelectionPage>,
        index: usize,
        quality: Quality,
    },
    Member {
        query: super::super::dto::MemberQuery,
        revision: String,
        section: Option<String>,
        index: usize,
        quality: Quality,
    },
    Album {
        id: String,
        start_id: Option<String>,
        revision: String,
        quality: Quality,
    },
    Podcast {
        scope: crate::podcast::PodcastScope,
        start_id: Option<String>,
        revision: String,
    },
    Local {
        query: crate::library::catalog::LocalQuery,
        start_id: Option<String>,
        revision: String,
    },
    MvRelated {
        mv_id: String,
        song_id: String,
        quality: Quality,
    },
    Cloud {
        query: crate::client::dto::CloudQuery,
        start_id: Option<String>,
        revision: String,
        quality: Quality,
    },
    Songs {
        ids: Vec<String>,
        index: usize,
        quality: Quality,
    },
    Playlist {
        id: String,
        start_id: Option<String>,
        revision: Option<String>,
        quality: Quality,
    },
    PlaylistBatch {
        id: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueueEdit {
    Select { entry_id: String },
    Remove { entry_ids: Vec<String> },
    Reorder { entry_ids: Vec<String> },
    Clear,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerAction {
    QueueFor {
        expected_queue_revision: String,
        edit: QueueEdit,
    },
    QueueDownloads {
        task_ids: Vec<String>,
        expected_queue_revision: String,
    },
    QueuePlaylistBatch {
        id: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueuePlaylist {
        id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    NextPlay {
        id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    Startup,
    Equalizer {
        expected_revision: String,
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
    RepairQuality,
    Play,
    Pause {
        paused: bool,
    },
    Seek {
        seconds: f64,
    },
    Volume {
        value: f64,
    },
    Speed {
        value: f64,
    },
    Next,
    Previous,
    Stop,
    Restore,
    Refresh,
    Quality {
        level: Quality,
    },
    QualityFor {
        play_id: String,
        expected_revision: String,
        level: Quality,
    },
    Mode {
        mode: PlaybackMode,
    },
    ModeFor {
        mode: PlaybackMode,
        expected_queue_revision: String,
        fm: bool,
    },
    Select {
        entry_id: String,
    },
    Remove {
        entry_ids: Vec<String>,
    },
    Reorder {
        entry_ids: Vec<String>,
    },
    Clear,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsView {
    pub word_lines: Vec<crate::lyrics::WordLine>,
    pub word_translation: Vec<LyricLine>,
    pub word_romanization: Vec<LyricLine>,
    pub epoch: String,
    pub play_id: String,
    pub lines: Vec<LyricLine>,
    pub translation: Vec<LyricLine>,
    pub romanization: Vec<LyricLine>,
    pub words: YrcInfo,
    pub offset: f64,
    pub scrollable: bool,
    pub display_type: DisplayType,
    pub preferences: LyricPreferences,
    pub network_failed: bool,
    pub from_cache: bool,
}
