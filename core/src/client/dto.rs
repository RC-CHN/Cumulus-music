pub use super::collections::{CollectionPageView, CollectionView, FollowCard, FollowUser};
pub use super::downloads::{
    DownloadCategory, DownloadCommand, DownloadFailure, DownloadJobView, DownloadQuery,
    DownloadResult, DownloadSort, DownloadTab, DownloadTaskView, DownloadsView, TaskControl,
};
pub use super::fm::{FmArtistView, FmCommand, FmFeedbackView, FmItemView, FmTrashView, FmView};
pub use super::history::{
    HistoryCommand, HistoryEntry, HistoryOperationView, HistoryPage, HistoryPlaylistCard,
    HistoryResource,
};
pub use super::local::{
    LocalCommand, LocalGroup, LocalGroupEntry, LocalLinkView, LocalMatchCommand, LocalMatchView,
    LocalPageView, LocalQuery, LocalRootView, LocalSort, LocalTrackView,
};
pub use super::media_preferences::{SystemMediaPreferencesCommand, SystemMediaPreferencesView};
pub use super::member::{
    MemberCard, MemberContent, MemberEntitlement, MemberModule, MemberPage, MemberPageView,
    MemberQuery, MemberRanking, MemberRightsView, MemberTarget,
};
pub use super::mv::{
    MvCard, MvDownloadChoice, MvLikeView, MvMutationView, MvPage, MvQuery, MvRecommendationsView,
    MvView, VideoAction, VideoView,
};
pub use super::player::{
    LyricsView, OutputCommand, OutputEngineView, OutputView, PlaySelection, PlayerAction,
    PlayerOrigin, PlayerView, QueueTrack, QueueView, StartupPreferencesCommand,
    StartupPreferencesView,
};
pub use super::playlist_cover::{CoverCommand, CoverOutcome};
pub use super::playlists::{PlaylistEditor, PlaylistMutationView, PlaylistOrderView, PlaylistTag};
pub use super::podcast::{
    EpisodeView, PodcastCard, PodcastCommand, PodcastMutationView, PodcastPage, PodcastQuery,
    PodcastState,
};
pub use super::rankings::{RankingPageView, RankingView, SongSaleView};
pub use super::relay::{
    RelayAlertView, RelayCommand, RelayOperationView, RelayPublicationCommand,
    RelayPublicationReceipt, RelayPublicationView, RelaySettingCommand, RelaySettingOperationView,
    RelaySettingsView, RelayView,
};
pub use super::scene_fm::{SceneFmCategoryView, SceneFmCommand, SceneFmView};
pub use super::search::{
    SearchCategory, SearchHistoryCommand, SearchItem, SearchResultQuery, SearchResultView,
    SearchSection,
};
pub use super::settings::{
    AccountSettingField, AccountSettingsCommand, AccountSettingsPage, LinkedPrivacy,
    SettingAvailability, SettingChange, SettingKey, SettingValue, SettingsOperationView,
    SettingsPhase,
};
pub use super::shortcut_preferences::{
    ShortcutEdit, ShortcutSettingsCommand, ShortcutSettingsView,
};
pub use super::together::{
    TogetherCommand, TogetherMemberView, TogetherOperationView, TogetherRoomView, TogetherView,
};
pub use super::window::{WindowPreferencesCommand, WindowPreferencesView};
pub use crate::account::collections::CollectionQuery;
use crate::account::{QrStatus, SessionStatus, resources::MutationPhase};
pub use crate::catalog::rankings::RankingQuery;
pub use crate::catalog::search::{SearchDiscovery, SearchDiscoveryQuery};
pub use crate::fm::discovery::{FmDiscoveryQuery, SavedFmDiscovery};
pub use crate::fm::{FmReason, FmReasons, FmSource};
pub use crate::history::HistoryKind;
pub use crate::playlist::cover::{CoverCrop, CoverImage, CoverTaskView};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub epoch: String,
    pub status: SessionStatus,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub can_sign_out: bool,
    pub login_notice: Option<crate::account::login_policy::LoginNotice>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountData<T> {
    pub epoch: String,
    pub value: T,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistView {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub description: String,
    pub creator: String,
    pub track_count: usize,
    pub owned: bool,
    pub favorite: bool,
    pub private: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistReference {
    /// Missing/zero IDs denote display-only credits, never a navigable artist.
    pub id: Option<String>,
    pub name: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    pub name: String,
    pub artists: Vec<String>,
    pub artist_refs: Vec<ArtistReference>,
    pub album: String,
    pub album_id: Option<String>,
    pub cover_url: Option<String>,
    pub duration_ms: u64,
    pub fee: i64,
    pub missing: bool,
    pub local: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPage {
    pub revision: Option<String>,
    pub tracks: Vec<TrackView>,
    pub total: usize,
    pub offset: usize,
    pub more: bool,
    pub playlist: Option<PlaylistView>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageRequest {
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub revision: Option<String>,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 100,
            revision: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrView {
    pub operation_id: String,
    pub status: QrStatus,
    /// Row-major module matrix. The host does not receive raw login tickets.
    pub modules: Vec<Vec<bool>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeResult {
    pub id: String,
    pub phase: MutationPhase,
    pub observed: Option<bool>,
}
pub use super::auth::{
    LoginAnswerView, LoginCaptchaView, LoginChallengeView, LoginCommand, LoginMode, LoginView,
    LogoutView, RenewalAction, RenewalState, SavedAccountView, SessionRenewalView,
};
pub use super::catalog::{
    AlbumCard, ArtistCard, CatalogDate, CatalogIntroduction, CatalogMembershipAction,
    CatalogMembershipOperation, CatalogMembershipView, CatalogPageView, CatalogResource,
    CatalogSelectionPage, CatalogVideoCard, CatalogVideoKind, CatalogView, ConcertCard,
};
pub use super::cloud::{
    CloudCommand, CloudDeleteView, CloudFailure, CloudPageView, CloudQuery, CloudResult, CloudSort,
    CloudTab, CloudTrackView, CloudUploadInput, CloudUploadView,
};
pub use super::comments::{
    CommentAuthorView, CommentFloorQuery, CommentFloorView, CommentOperationView, CommentPageView,
    CommentQuery, CommentReplyView, CommentResource, CommentSort, CommentView, CommentWriteAction,
    CommentWriteCommand, CommentWritePhase, CommentWriteView,
};
pub use super::likes::{LikeOperationView, LikeRecoveryCommand, LikeRecoveryView};
pub use crate::catalog::details::CatalogQuery;

pub use super::mini_preferences::{MiniPreferencesCommand, MiniPreferencesView};

pub use super::desktop_lyric_preferences::{
    DesktopLyricPreferencesCommand, DesktopLyricPreferencesView,
};
