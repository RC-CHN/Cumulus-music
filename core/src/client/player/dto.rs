use crate::{
    client::dto::TrackView,
    lyrics::{DisplayType, LyricLine, LyricPreferences, YrcInfo},
    membership::Quality,
    playback::{
        PlaybackMode, TimelineSnapshot,
        controller::{PlaybackSpeedScope, PlaybackStatus},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub resource_kind: Option<PlayerResourceKind>,
    pub audiobook_skip_available: bool,
    pub fm: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_fm_rule_id: Option<String>,
    /// Display state only. Never expose the native session, backup or algorithm map.
    pub intelligence: Option<crate::playback::intelligence::IntelligencePhase>,
    /// The host-supplied operation UUID, never the private native session id.
    pub intelligence_operation: Option<String>,
    pub equalizer: EqualizerView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_parametric: Option<crate::playback::controller::ParametricBindingStatus>,
    /// PCM observed for the configured component on the current playing entry.
    pub parametric_processing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_parametric: Option<crate::playback::controller::DeviceParametricView>,
    pub loudness: LoudnessView,
    pub epoch: String,
    pub revision: String,
    pub queue_revision: String,
    pub queue_more_available: bool,
    pub queue_more_tip_eligible: bool,
    pub state: PlaybackStatus,
    /// Desired first presentation state while a decoder is pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_paused: Option<bool>,
    pub play_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_minibar_receipt: Option<String>,
    pub entry_id: Option<String>,
    /// Local download task identity only; never expose the media file locator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_task_id: Option<String>,
    pub track: Option<TrackView>,
    /// Bounded plain text from the selected song's classic title metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_alias: Option<String>,
    /// Public episode header metadata bound to this snapshot's native play ID,
    /// episode and main song. Raw podcast records never cross the boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_detail: Option<super::PlayerVoiceDetail>,
    pub origin: Option<PlayerOrigin>,
    /// Display text frozen by the selected native album, artist or playlist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_name: Option<String>,
    /// Known non-navigable playback provenance; no raw source data is exposed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub vinyl_similar_origin: bool,
    /// The selected catalog entry came from search; this label has no route.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub search_origin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_rank_playlist_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation_origin: Option<RecommendationOrigin>,
    pub position: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffered_through: Option<f64>,
    pub native_seeking: bool,
    pub timeline: Option<TimelineSnapshot>,
    pub quality_preferences: QualityPreferencesView,
    pub can_change_quality: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub catalog_file_source: bool,
    pub requested_quality: Option<String>,
    pub actual_quality: Option<String>,
    pub volume: f64,
    pub unmute_volume: f64,
    pub speed: f64,
    pub speed_scope: PlaybackSpeedScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_speed: Option<crate::playback::controller::SongSpeedView>,
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
/// The physical index click and body double click both use the official
/// table's double-click action, but only the index offers a transport toggle.
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchPodcastRowGesture {
    Index,
    DoubleClick,
    Menu,
    Next,
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
/// Public identity of the recommendation snapshot that created this entry.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecommendationOrigin {
    Daily { revision: String },
    Style { revision: String },
}
#[derive(Clone, Serialize)]
pub struct EqualizerView {
    pub revision: String,
    pub settings: crate::effects::equalizer::EqualizerSettings,
    pub preset: Option<crate::effects::equalizer::EqualizerPreset>,
    pub saved_custom: Option<[f32; 10]>,
    pub status: crate::effects::equalizer::EqualizerStatus,
    pub spatial_status: crate::effects::equalizer::EqualizerSpatialStatus,
    pub damaged: bool,
    pub source_bypassed: bool,
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
/// A whitelisted route identity stored on this individual queue row.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueueOrigin {
    Album { id: String },
    Artist { id: String },
    Playlist { id: String },
    Local,
    Cloud,
    Downloads,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueTrack {
    pub entry_id: String,
    pub track: TrackView,
    pub more_recommendation: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<QueueOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub like_song_id: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub catalog_menu: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub voice_menu: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub local_menu: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub collectible: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueCatalogMenu {
    pub epoch: String,
    pub revision: String,
    pub entry_id: String,
    pub track: TrackView,
    pub source_label: String,
    pub origin: Option<QueueOrigin>,
    pub comment_count: Option<u64>,
    pub downloaded: bool,
    pub download_task_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueVoiceMenu {
    pub epoch: String,
    pub revision: String,
    pub entry_id: String,
    pub track: TrackView,
    pub voice_id: String,
    pub source_label: String,
    pub radio_id: Option<String>,
    pub creator: Option<super::PlayerVoiceCreator>,
    pub comment_count: Option<u64>,
    pub downloaded: Option<bool>,
    pub directory_available: bool,
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
    pub more_recommendations: Option<crate::playback::queue_more::QueueMoreView>,
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
    DailyRecommendation {
        revision: String,
        index: usize,
        track_id: String,
        quality: Quality,
    },
    AlbumDetail {
        album: super::super::catalog::AlbumPlaybackScope,
        row: super::super::catalog::AlbumPlaybackRow,
        quality: Quality,
    },
    LatestRelease {
        area: super::super::catalog::NewSongArea,
        revision: String,
        start_id: String,
        quality: Quality,
    },
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
    AlbumDetail {
        album: super::super::catalog::AlbumPlaybackScope,
        row: Option<super::super::catalog::AlbumPlaybackRow>,
        quality: Quality,
    },
    VerticalZonePlaylist {
        id: String,
        view_id: String,
        card_key: String,
        quality: Quality,
    },
    VerticalZonePlaylistDetail {
        id: String,
        receipt: String,
        revision: String,
        start_id: Option<String>,
        double_click: bool,
        quality: Quality,
    },
    ProfileRankPlaylist {
        uid: Option<String>,
        quality: Quality,
    },
    FollowLatest {
        revision: String,
        quality: Quality,
    },
    SongRowDoubleClick {
        selection: SongRowSelection,
    },
    Voice {
        id: String,
    },
    PodcastSubscriptionRecommendation {
        receipt: String,
        revision: String,
        id: String,
        #[serde(default)]
        action: PodcastRecommendationAction,
    },
    PodcastRecent {
        revision: String,
        id: String,
        double_click: bool,
    },
    Downloads {
        query: crate::client::dto::DownloadQuery,
        start_id: Option<String>,
    },
    ListenedBookChapter {
        revision: String,
        key: usize,
    },
    HistoryRadioChapter {
        history_kind: crate::history::HistoryKind,
        revision: String,
        id: String,
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
    LocalBatch {
        query: crate::library::catalog::LocalQuery,
        revision: String,
        tracks: Vec<crate::client::dto::LocalTrackReference>,
    },
    LocalBatchAdd {
        query: crate::library::catalog::LocalQuery,
        revision: String,
        tracks: Vec<crate::client::dto::LocalTrackReference>,
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
    MessageTrack {
        id: String,
        quality: Quality,
    },
    MessageVoice {
        id: String,
    },
    SearchArtistCard {
        id: String,
        quality: Quality,
    },
    SearchPodcastCard {
        id: String,
        receipt: String,
    },
    SearchPodcastRow {
        id: String,
        receipt: String,
        gesture: SearchPodcastRowGesture,
    },
    PodcastBrowseVoice {
        receipt: String,
    },
    SearchResultVoice {
        id: String,
    },
    SearchVoiceMenu {
        target: crate::client::dto::SearchVoiceMenuTarget,
    },
    SearchHotTrack {
        id: String,
        quality: Quality,
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
    VerticalZonePlaylistBatch {
        id: String,
        receipt: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
    },
    PlaylistBatch {
        id: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
    },
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PodcastRecommendationAction {
    #[default]
    Overlay,
    Play,
    Next,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueueEdit {
    Select {
        entry_id: String,
    },
    Remove {
        entry_ids: Vec<String>,
    },
    Reorder {
        entry_ids: Vec<String>,
    },
    Move {
        entry_ids: Vec<String>,
        movement: crate::playback::QueueMovement,
    },
    Clear,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistDropRow {
    pub track_id: String,
    pub track_index: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlbumDropRow {
    pub track_id: String,
    pub track_index: usize,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerAction {
    StepMiniBar {
        play_id: String,
        expected_queue_revision: String,
        direction: crate::playback::Step,
    },
    StepEndedMiniBar {
        receipt: String,
        entry_id: String,
        expected_queue_revision: String,
        direction: crate::playback::Step,
    },
    QueueMoreOpened {
        expected_revision: String,
    },
    QueueMoreToggle {
        expected_revision: String,
        enabled: bool,
    },
    QueueFor {
        expected_queue_revision: String,
        #[serde(default)]
        expected_room_id: Option<String>,
        edit: QueueEdit,
    },
    QueueLocal {
        query: crate::library::catalog::LocalQuery,
        revision: String,
        #[serde(default)]
        track_id: Option<String>,
        expected_queue_revision: String,
    },
    DropLocalIntoQueue {
        query: crate::library::catalog::LocalQuery,
        revision: String,
        track_id: String,
        expected_queue_revision: String,
        expected_play_id: Option<String>,
        before_entry_id: Option<String>,
    },
    DropPlaylistSongIntoQueue {
        id: String,
        revision: String,
        track_id: String,
        track_index: usize,
        quality: Quality,
        expected_queue_revision: String,
        expected_play_id: Option<String>,
        before_entry_id: Option<String>,
    },
    DropPlaylistSongsIntoQueue {
        id: String,
        revision: String,
        rows: Vec<PlaylistDropRow>,
        quality: Quality,
        expected_queue_revision: String,
        expected_play_id: Option<String>,
        before_entry_id: Option<String>,
    },
    DropSearchSongsIntoQueue {
        rows: Vec<super::super::search::SearchDropRow>,
        quality: Quality,
        expected_queue_revision: String,
        expected_play_id: Option<String>,
        before_entry_id: Option<String>,
    },
    DropAlbumSongsIntoQueue {
        id: String,
        revision: String,
        rows: Vec<AlbumDropRow>,
        quality: Quality,
        expected_queue_revision: String,
        expected_play_id: Option<String>,
        before_entry_id: Option<String>,
    },
    QueueDownloads {
        task_ids: Vec<String>,
        expected_queue_revision: String,
    },
    QueueVerticalZonePlaylistAll {
        id: String,
        receipt: String,
        revision: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueVerticalZonePlaylistNext {
        id: String,
        receipt: String,
        revision: String,
        song_id: String,
        quality: Quality,
        expected_queue_revision: String,
    },
    QueueVerticalZonePlaylistBatch {
        id: String,
        receipt: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
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
    QueueVoice {
        id: String,
        #[serde(default)]
        program: Option<crate::client::dto::PodcastProgramTarget>,
        expected_queue_revision: String,
    },
    QueueRecentProgram {
        target: crate::client::dto::PodcastRecentProgramTarget,
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
    QueueArtistSongsBatch {
        pages: Vec<super::super::catalog::CatalogSelectionPage>,
        ids: Vec<String>,
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
    QueueAlbumDetail {
        album: super::super::catalog::AlbumPlaybackScope,
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
    SpeedFor {
        scope: PlaybackSpeedScope,
        value: f64,
    },
    SongSpeed {
        expected_revision: String,
        expected_entry: String,
        selection: crate::playback::controller::SongSpeedSelection,
    },
    Next,
    Previous,
    StepLoading {
        play_id: String,
        expected_queue_revision: String,
        direction: crate::playback::Step,
    },
    Stop,
    Restore,
    Refresh,
    RefreshFor {
        play_id: String,
        expected_queue_revision: String,
    },
    PlayMissingDownloadOnline {
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
pub struct LyricContributorView {
    pub nickname: String,
    pub user_id: Option<String>,
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
    pub lyric_contributor: Option<LyricContributorView>,
    pub translation_contributor: Option<LyricContributorView>,
    pub words: YrcInfo,
    pub offset: f64,
    pub scrollable: bool,
    pub display_type: DisplayType,
    pub preferences: LyricPreferences,
    pub network_failed: bool,
    pub is_cloud_lyric: bool,
    pub from_cache: bool,
    pub from_sidecar: bool,
    pub from_embedded: bool,
}
