pub use super::account_binding_qr::AccountBindingQrView;
pub use super::collections::{CollectionPageView, CollectionView, FollowCard, FollowUser};
pub use super::downloads::{
    DownloadCategory, DownloadCommand, DownloadFailure, DownloadJobView, DownloadQuery,
    DownloadResult, DownloadSort, DownloadTab, DownloadTaskView, DownloadsView, SongDownloadState,
    TaskControl,
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
pub use super::login_device_removal::{
    LoginDeviceRemovalCommand, LoginDeviceRemovalPhase, LoginDeviceRemovalView,
};
pub use super::login_devices::{LoginDeviceLimitsView, LoginDeviceView, LoginDevicesView};
pub use super::media_preferences::{SystemMediaPreferencesCommand, SystemMediaPreferencesView};
pub use super::member::{
    MemberCard, MemberCardOffer, MemberContent, MemberEntitlement, MemberModule, MemberPage,
    MemberPageView, MemberQuery, MemberRanking, MemberRightsView, MemberSignAction, MemberSignCard,
    MemberSignCommand, MemberSignDetail, MemberSignOutcome, MemberSignPhase, MemberTarget,
};
pub use super::mv::{
    MvCard, MvDownloadChoice, MvLikeView, MvMutationView, MvPage, MvQuery, MvRecommendationsView,
    MvView, VideoAction, VideoView,
};
pub use super::player::{
    IntelligenceCommand, IntelligenceEligibility, IntelligenceOrigin, LyricsView, OutputCommand,
    OutputEngineView, OutputView, PlaySelection, PlayerAction, PlayerOrigin, PlayerResourceKind,
    PlayerView, QualityChoiceView, QualityDetailsView, QualityTrialCommand, QualityTrialOutcome,
    QualityTrialView, QueueClearContext, QueueClickPreferencesCommand, QueueClickPreferencesView,
    QueueCollectionView, QueueEdit, QueueTrack, QueueView, SearchResultPlayback, SongRowSelection,
    StartupPreferencesCommand, StartupPreferencesView,
};
pub use super::playlist_cover::{CoverCommand, CoverOutcome};
pub use super::playlist_creator::PlaylistCreatorProfile;
pub use super::playlist_essential::{PlaylistEssentialCategory, PlaylistEssentialPage};
pub use super::playlist_square::PlaylistSquarePage;
pub use super::playlist_tags_browse::{
    PlaylistTagGroup, PlaylistTagPage, PlaylistTagQuery, PlaylistTopTags,
};
pub use super::playlists::{PlaylistEditor, PlaylistMutationView, PlaylistOrderView, PlaylistTag};
pub use super::podcast::{
    EpisodeView, PodcastCard, PodcastCatalogue, PodcastCatalogueQuery, PodcastCategory,
    PodcastCommand, PodcastHomeBlock, PodcastHomePage, PodcastMutationView, PodcastPage,
    PodcastQuery, PodcastRankGroup, PodcastRankMenu, PodcastRankQuery, PodcastRankResource,
    PodcastRankRow, PodcastRankTab, PodcastRanks, PodcastSearchMembership, PodcastState,
    PodcastSubcategory, PodcastTopLayout,
};
pub use super::profile_relations::{
    ProfileRelationAccess, ProfileRelationEntry, ProfileRelationKind, ProfileRelationPage,
};
pub use super::rankings::{RankingPageView, RankingView, SongSaleView};
pub use super::relay::{
    RelayAlertView, RelayCommand, RelayOperationView, RelayPublicationCommand,
    RelayPublicationReceipt, RelayPublicationView, RelaySettingCommand, RelaySettingOperationView,
    RelaySettingsView, RelayView,
};
pub use super::scene_fm::{SceneFmCategoryView, SceneFmCommand, SceneFmView};
pub use super::search::{
    ReasonTarget, SearchCategory, SearchHistoryCommand, SearchItem, SearchReasonPage,
    SearchResultQuery, SearchResultView, SearchSection,
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
    TogetherCommand, TogetherMemberView, TogetherOperationView, TogetherRoomView,
    TogetherStatisticsView, TogetherView,
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
    pub creator_id: Option<String>,
    pub creator_avatar_url: Option<String>,
    pub header_date: Option<PlaylistHeaderDate>,
    pub tags: Vec<String>,
    pub track_count: usize,
    pub subscribed_count: Option<u64>,
    pub play_count: Option<u64>,
    pub owned: bool,
    pub favorite: bool,
    pub private: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistCollectSort {
    Default,
    Recently,
}
impl PlaylistCollectSort {
    pub(super) fn wire(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Recently => "recently",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCollectPage {
    pub playlists: Vec<PlaylistView>,
    pub next_page: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistHeaderDate {
    pub kind: PlaylistHeaderDateKind,
    pub timestamp_ms: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistHeaderDateKind {
    Created,
    Updated,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSubscriberView {
    pub user_id: Option<String>,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub signature: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSubscribersPage {
    pub subscribers: Vec<PlaylistSubscriberView>,
    pub previous_offset: Option<u32>,
    pub next_offset: Option<u32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<DailyRecommendationState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style_tags: Option<super::style_tags::StyleCategory>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRecommendationState {
    pub cached: bool,
    /// Recovered presentation flag; never an entitlement decision.
    pub show_free_tag: bool,
    pub birthday_today: bool,
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
    pub poll_after_ms: u32,
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
pub use super::catalog::{ArtistGalleryPage, ArtistGalleryQuery, FollowNewSongPage};
pub use super::cloud::{
    CloudCommand, CloudDeleteView, CloudFailure, CloudPageView, CloudQuery, CloudRemovalFileState,
    CloudRemovalFileView, CloudRemovalPhase, CloudRemovalView, CloudResourceView, CloudResult,
    CloudSort, CloudTab, CloudTrackView, CloudUploadInput, CloudUploadRemovalItem,
    CloudUploadRemovalView, CloudUploadView, PlannedCloudUploadInput,
};
pub use super::comments::{
    CommentAuthorView, CommentFloorQuery, CommentFloorView, CommentOperationView, CommentPageView,
    CommentQuery, CommentReplyView, CommentResource, CommentResourceCard, CommentResourceView,
    CommentSort, CommentView, CommentWriteAction, CommentWriteCommand, CommentWritePhase,
    CommentWriteView,
};
pub use super::likes::{LikeOperationView, LikeRecoveryCommand, LikeRecoveryView};
pub use crate::catalog::details::CatalogQuery;

pub use super::mini_preferences::{MiniPreferencesCommand, MiniPreferencesView};

pub use super::account_avatar::AccountAvatarCommand;
pub use super::account_profile::AccountProfileEditCommand;
pub use super::auth::LoginPresentation;
pub use super::auth::{ServerQuickLoginCommand, ServerQuickLoginOperationView};
pub use super::desktop_lyric_preferences::{
    DesktopLyricPreferencesCommand, DesktopLyricPreferencesView,
};
pub use super::player::LyricReportView;
pub use crate::account::login::presentation::{LoginCountry, LoginDocument};
pub use crate::account::profile::avatar::{AvatarImage, AvatarTaskView};
pub use crate::account::profiles::{QuickLoginAccountView, QuickLoginEligibilityView};
