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
    pub resource_kind: Option<PlayerResourceKind>,
    pub fm: bool,
    /// Display state only. Never expose the native session, backup or algorithm map.
    pub intelligence: Option<crate::playback::intelligence::IntelligencePhase>,
    /// The host-supplied operation UUID, never the private native session id.
    pub intelligence_operation: Option<String>,
    pub equalizer: EqualizerView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_parametric: Option<crate::playback::controller::ParametricBindingStatus>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffered_through: Option<f64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<&'static str>,
    pub error_retryable: Option<bool>,
    pub history_warning: Option<&'static str>,
    pub position_reset: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerResourceKind {
    Catalog,
    Cloud,
    Local,
    Downloaded,
    Voice,
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
    pub preset: Option<crate::effects::equalizer::EqualizerPreset>,
    pub saved_custom: Option<[f32; 10]>,
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
    pub clear_context: QueueClearContext,
    pub epoch: String,
    pub revision: String,
    pub current: Option<String>,
    pub tracks: Vec<QueueTrack>,
}
/// Local room policy for the displayed confirmation. No chat/RTC credentials.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueClearContext {
    pub room_id: Option<String>,
    pub issue: Option<&'static str>,
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

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultPlayback {
    SongTabAll,
    ComprehensiveAll,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SongRowSelection {
    Catalog {
        pages: Vec<super::super::catalog::CatalogSelectionPage>,
        index: usize,
        quality: Quality,
    },
    Playlist {
        id: String,
        start_id: String,
        revision: Option<String>,
        quality: Quality,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaySelection {
    SongRowDoubleClick {
        selection: SongRowSelection,
    },
    Voice {
        id: String,
    },
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
    StyleRecommendations {
        revision: String,
        index: usize,
        quality: Quality,
    },
    DailyRecommendations {
        revision: String,
        index: usize,
        quality: Quality,
    },
    DailyRecommendationsBatch {
        revision: String,
        indices: Vec<usize>,
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
    CloudBatch {
        ids: Vec<String>,
        revision: String,
        quality: Quality,
        #[serde(default)]
        query: Option<crate::client::dto::CloudQuery>,
    },
    SearchResultVoice {
        id: String,
    },
    SearchResultTrack {
        id: String,
        quality: Quality,
        toggle_current: bool,
    },
    SearchResults {
        ids: Vec<String>,
        index: usize,
        quality: Quality,
        context: SearchResultPlayback,
    },
    SearchChart {
        ids: Vec<String>,
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
        #[serde(default)]
        expected_room_id: Option<String>,
        edit: QueueEdit,
    },
    QueueLocal {
        query: crate::library::catalog::LocalQuery,
        revision: String,
        expected_queue_revision: String,
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
    QueueCloudBatch {
        ids: Vec<String>,
        revision: String,
        quality: Quality,
        expected_queue_revision: String,
        #[serde(default)]
        query: Option<crate::client::dto::CloudQuery>,
    },
    QueuePlaylist {
        id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueDailyRecommendations {
        revision: String,
        #[serde(default)]
        indices: Option<Vec<usize>>,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueStyleRecommendations {
        revision: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueArtistSongs {
        pages: Vec<super::super::catalog::CatalogSelectionPage>,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueAlbumBatch {
        id: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueAlbum {
        id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    NextPlay {
        id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    SearchNextPlay {
        id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    Startup,
    Equalizer {
        expected_revision: String,
        settings: crate::effects::equalizer::EqualizerSettings,
        preset: Option<crate::effects::equalizer::EqualizerPreset>,
    },
    SaveCustomEqualizer {
        expected_revision: String,
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
    PauseFor {
        play_id: String,
        expected_queue_revision: String,
        paused: bool,
    },
    Seek {
        seconds: f64,
    },
    SeekFor {
        play_id: String,
        expected_queue_revision: String,
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
    RefreshFor {
        play_id: String,
        expected_queue_revision: String,
    },
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
    pub from_sidecar: bool,
    pub from_embedded: bool,
}
