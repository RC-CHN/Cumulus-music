import type { LocalNetworkReport } from './networkDiagnostics';
import type { PackageVerification, UpdateCheck } from './updates';
import type { ExternalPlaylistImportClient } from './externalPlaylistImport';
import type { MemberWelfareClient } from './memberWelfare';
import type { RemoteSkinClient } from './remoteSkins';
import type { CommunityTopicClient } from './communityTopic';
import type { MessageClient } from './messages';
import type { ArtistOnboardingClient } from './artistOnboarding';
import type { SearchClient, SearchResultsClient, SearchSongShareClient } from './search';
import type { PcPresentationClient } from './pcPresentation';
import type { NativeLikesClient } from './nativeLikes';
import type { LikeRecoveryClient } from './likeRecovery';
import type { PlayerClient } from './player';
import type { VoiceDetailActionsClient } from './voicePage';
import type { AudiobookSkipClient } from './audiobookSkip';
import type { PlayerPodcastResumeClient } from './playerPodcastResume';
import type { PlaylistClient } from './playlists';
import type { SongRowCommentClient, SongRowCommentMenu } from './songRowCommentMenu';
import type { PlaylistCoverClient } from './playlistCover';
import type { ColdOfflineClient } from './coldOffline';
import type { DownloadClient, SongDownloadState } from './downloads';
import type { CloudClient } from './cloud';
import type { MvClient } from './mv';
import type { CommentClient, CommentReportClient } from './comments';
import type {
  CommunityClient,
  CommunityDeleteClient,
  CommunityReportClient,
  CommunityNoticeClient,
  ProfileNotesClient,
} from './community';
import type { CommentResourceClient } from './commentResource';
import type { CommentWriteClient, VinylCommentWriteClient } from './commentWrites';
import type { CommentFloorClient } from './commentFloor';
import type { LocalClient, LocalCollectClient, LocalDirectoryClient } from './local';
import type { LocalMatchClient } from './localMatching';
import type {
  PodcastCatalogueClient,
  PodcastClient,
  PodcastHomeClient,
  PodcastLikedClient,
  PodcastProgramMenuClient,
  PodcastRankingClient,
  PodcastRecentClient,
  PodcastSearchClient,
} from './podcast';
import type { ProfileBlacklistClient, ProfileBlacklistHeader } from './profileBlacklist';
import type { ProfileReportClient } from './profileReport';
import type { ProfileFollowClient, ProfileFollowView } from './profileFollow';
import type { ProfilePodcastClient } from './profilePodcasts';
import type { TogetherClient } from './together';
import type { AuthClient } from './auth';
import type { RankingClient } from './rankings';
import type { FmClient } from './fm';
import type { SceneFmAutoplayClient, SceneFmClient } from './sceneFm';
import type { SceneFmPaletteClient } from './sceneFmPalette';
import type { MemberClient, MemberPortalClient } from './member';
import type { CatalogClient } from './catalog';
import type { OrdinaryEssentialClient } from './ordinaryEssential';
import type { VerticalZoneClient } from './verticalZone';
import type { DiscoveryHomeClient } from './discoveryHome';
import type { FollowNewSongClient, ReleaseNotificationClient } from './followNewSong';
import type { LatestReleaseClient } from './latestReleases';
import type { CollectionClient } from './collections';
import type {
  HistoryClient,
  HistoryRadioCollectionClient,
  HistoryRowMenuClient,
  HistorySyncClient,
} from './history';
import type { SettingsClient } from './settings';
import type { LoginDeviceRemovalClient } from './loginDeviceRemoval';
import type { AccountDetailClient, AccountRegionTable } from './accountDetail';
import type { AccountAvatarClient } from './accountAvatar';
import type { AccountProfileEditClient } from './accountProfileEdit';
import type { QueueClickPreferencesClient } from './queueClickPreferences';
import type { StartupPreferencesClient } from './startup';
import type { PlaybackPowerClient } from './playbackPower';
import type { PlaybackCacheClient } from './playbackCache';
import type { AutostartClient, WindowPreferencesClient, WindowLifecycleClient } from './window';
import type { SystemMediaClient } from './systemMedia';
import type { ShortcutsClient } from './shortcuts';
import type { OutputClient } from './output';
import type { DailyNotificationClient } from './dailyNotification';
import type {
  SystemMasterVolumeClient,
  SystemSpatialSoundClient,
  VolumeDiagnosisClient,
} from './systemMasterVolume';
import type { EffectDirectoryClient } from './effectDirectory';
import type { RelayClient } from './relay';
import type { RelayPublicationClient } from './relayPublication';
import type { ProfilePlayRecordClient } from './profilePlayRecord';
import type { ProfileRelationClient } from './profileRelations';
/** Wire format returned by core::AppInfo. Keep the IPC test in sync. */
export interface AppInfo {
  name: string;
  coreVersion: string;
  protocolVersion: number;
}
export interface StyleSeedHandle {
  revision: string;
  id: string;
}
export interface StyleSeedPage {
  revision: string;
  cards: { id: string; title: string; coverUrl: string | null }[];
}
export interface StyleCategory {
  id: string;
  name: string;
  tags: { id: string; name: string }[];
}
export interface StyleTagOperation {
  id: string;
  expectedRevision: string;
  categoryId: string;
  tagId: string;
  phase:
    | 'pending'
    | 'unchanged'
    | 'confirmed'
    | 'rejected'
    | 'uncertain'
    | 'accepted_current_state';
  observed: { categoryId: string; tagIds: string[]; revision: string } | null;
}

/** UI-facing use cases; no Tauri types or raw HTTP API in this interface. */
export interface CoreClient
  extends ArtistOnboardingClient,
    PcPresentationClient,
    SearchClient,
    SearchResultsClient,
    SearchSongShareClient,
    DiscoveryHomeClient,
    VerticalZoneClient,
    OrdinaryEssentialClient,
    LikeRecoveryClient,
    NativeLikesClient,
    PlayerClient,
    VoiceDetailActionsClient,
    PlaylistClient,
    SongRowCommentClient,
    ExternalPlaylistImportClient,
    MemberWelfareClient,
    RemoteSkinClient,
    PlaylistCoverClient,
    DownloadClient,
    ColdOfflineClient,
    CloudClient,
    MvClient,
    CommentClient,
    CommentReportClient,
    MessageClient,
    CommunityClient,
    CommunityTopicClient,
    CommunityDeleteClient,
    CommunityReportClient,
    ProfileNotesClient,
    CommunityNoticeClient,
    CommentResourceClient,
    CommentWriteClient,
    VinylCommentWriteClient,
    CommentFloorClient,
    LocalClient,
    LocalCollectClient,
    LocalDirectoryClient,
    LocalMatchClient,
    PodcastClient,
    AudiobookSkipClient,
    PlayerPodcastResumeClient,
    PodcastCatalogueClient,
    PodcastHomeClient,
    PodcastLikedClient,
    PodcastProgramMenuClient,
    PodcastRankingClient,
    PodcastRecentClient,
    PodcastSearchClient,
    ProfilePodcastClient,
    ProfileFollowClient,
    ProfileBlacklistClient,
    ProfileReportClient,
    TogetherClient,
    AuthClient,
    RankingClient,
    FmClient,
    SceneFmClient,
    SceneFmPaletteClient,
    SceneFmAutoplayClient,
    MemberClient,
    MemberPortalClient,
    CatalogClient,
    FollowNewSongClient,
    LatestReleaseClient,
    ReleaseNotificationClient,
    CollectionClient,
    HistoryClient,
    HistoryRadioCollectionClient,
    HistoryRowMenuClient,
    HistorySyncClient,
    SettingsClient,
    AccountDetailClient,
    AccountAvatarClient,
    LoginDeviceRemovalClient,
    AccountProfileEditClient,
    StartupPreferencesClient,
    PlaybackPowerClient,
    PlaybackCacheClient,
    QueueClickPreferencesClient,
    OutputClient,
    SystemMasterVolumeClient,
    SystemSpatialSoundClient,
    DailyNotificationClient,
    VolumeDiagnosisClient,
    EffectDirectoryClient,
    AutostartClient,
    WindowPreferencesClient,
    SystemMediaClient,
    ShortcutsClient,
    WindowLifecycleClient,
    RelayClient,
    RelayPublicationClient,
    ProfilePlayRecordClient,
    ProfileRelationClient {
  searchPresentation: NonNullable<SearchResultsClient['searchPresentation']>;
  getAppInfo(): Promise<AppInfo>;
  checkForUpdates(): Promise<UpdateCheck>;
  openUpdateReleasePage(): Promise<void>;
  verifyUpdatePackage(): Promise<PackageVerification | null>;
  localNetworkDiagnostics(): Promise<LocalNetworkReport>;
  copyText(text: string): Promise<void>;
  songDownloadState(epoch: string, id: string): Promise<SongDownloadState>;
  voiceDownloadState(epoch: string, id: string): Promise<SongDownloadState>;
  accountSnapshot(): Promise<AccountView>;
  restoreAccount(): Promise<AccountView>;
  beginQr(): Promise<QrView>;
  beginClassicRemoveGuideQr(epoch: string, ticket: string, retry: boolean): Promise<QrView>;
  dismissClassicRemoveGuide(epoch: string, ticket: string): Promise<void>;
  pollQr(operationId: string): Promise<QrStatus>;
  qrIdentityPending(operationId: string): Promise<boolean>;
  reconcileQr(operationId: string): Promise<void>;
  cancelQr(operationId: string): Promise<void>;
  accountProfileRegions(epoch: string): Promise<AccountData<AccountRegionTable | null>>;
  playlists(): Promise<AccountData<PlaylistView[]>>;
  playlistCollectPage(
    epoch: string,
    sort: 'default' | 'recently',
    page: number,
  ): Promise<AccountData<{ playlists: PlaylistView[]; nextPage: number | null }>>;
  playlistCreatorProfile(
    epoch: string,
    uid: string,
  ): Promise<AccountData<PlaylistCreatorProfile | null>>;
  playlistTagPage(epoch: string, query: PlaylistTagQuery): Promise<AccountData<PlaylistTagPage>>;
  playlistTagCatalogue(epoch: string): Promise<AccountData<PlaylistTagGroup[]>>;
  playlistTopTags(epoch: string): Promise<AccountData<PlaylistTopTags>>;
  playlistEssentialCategories(epoch: string): Promise<AccountData<PlaylistEssentialCategory[]>>;
  playlistEssentialPage(
    epoch: string,
    category: string,
    continuation?: string,
  ): Promise<AccountData<PlaylistEssentialPage>>;
  playlistSquareRecommendations(
    epoch: string,
    continuation?: string,
  ): Promise<AccountData<PlaylistSquarePage>>;
  playlistSubscribers(
    epoch: string,
    id: string,
    offset: number,
  ): Promise<AccountData<PlaylistSubscribersPage>>;
  playlistPage(id: string, page: PageRequest, songMenu?: true): Promise<AccountData<TrackPage>>;
  playlistDurationPrepare(
    epoch: string,
    id: string,
    source: import('./playlistDuration').PlaylistDurationSource,
  ): Promise<AccountData<import('./playlistDuration').PlaylistDurationPage>>;
  mutatePlaylistLocal(
    epoch: string,
    id: string,
    revision: string,
    change: PlaylistLocalChange,
  ): Promise<AccountData<{ changed: boolean }>>;
  searchSongs(keyword: string, page: PageRequest): Promise<AccountData<TrackPage>>;
  dailySongs(): Promise<AccountData<TrackPage>>;
  styleRecommendations(epoch: string): Promise<AccountData<TrackPage>>;
  restoreStyleRecommendations(
    epoch: string,
    revision: string,
    seed: StyleSeedHandle | null,
  ): Promise<AccountData<TrackPage> | null>;
  styleHomeSeeds(epoch: string): Promise<AccountData<StyleSeedPage>>;
  seededStyleRecommendations(epoch: string, seed: StyleSeedHandle): Promise<AccountData<TrackPage>>;
  styleRecommendationCategories(epoch: string): Promise<AccountData<StyleCategory[]>>;
  styleTagOperations(epoch: string, id?: string): Promise<AccountData<StyleTagOperation[]>>;
  saveStyleTag(
    epoch: string,
    id: string,
    expectedRevision: string,
    categoryId: string,
    tagId: string,
  ): Promise<AccountData<StyleTagOperation>>;
  reconcileStyleTag(epoch: string, id: string): Promise<AccountData<StyleTagOperation>>;
  acceptCurrentStyleTag(
    epoch: string,
    id: string,
    observedRevision: string,
  ): Promise<AccountData<StyleTagOperation>>;
  likedIds(): Promise<AccountData<string[]>>;
  setLiked(epoch: string, id: string, liked: boolean): Promise<AccountData<LikeResult>>;
}

export interface AccountView {
  epoch: string;
  status: 'unknown' | 'authenticated' | 'expired' | 'signed_out';
  nickname: string | null;
  avatarUrl: string | null;
  profileShortcut?: {
    id: string;
    notes: number | null;
    following: number | null;
    followers: number | null;
  };
  /** Additive native capability; absent on older core versions. */
  canSignOut?: boolean;
  classicRemoveGuide?: string;
  liveJskExpiry?: boolean;
  loginNotice?: {
    kind: 'expired' | 'reauthenticate';
    title: string | null;
    message: string;
  } | null;
}
export interface AccountData<T> {
  epoch: string;
  value: T;
}
export interface PageRequest {
  offset: number;
  limit: number;
  revision?: string | null;
  localRevision?: string;
}
export interface PlaylistLocalReference {
  id: string;
  revision: string;
}
export interface PlaylistLocalChange {
  op: 'add' | 'remove';
  rows: PlaylistLocalReference[];
}
export interface PlaylistCreatorProfile {
  id: string;
  nickname: string;
  avatarUrl: string | null;
  signature: string;
  notesCount?: number | null;
  podcasts?: { isSelf: boolean; total: number } | null;
  listeningRank?: { isSelf: boolean; listenSongs: number | null } | null;
  followingCount?: number | null;
  followersCount?: number | null;
  playlistsVisible?: boolean;
  follow?: ProfileFollowView | null;
  blacklist?: ProfileBlacklistHeader | null;
  created: PlaylistView[];
  collected: PlaylistView[];
}

export type PlaylistTagQuery = { kind: 'category'; tag: string } | { kind: 'items'; ids: string[] };
export interface PlaylistTagPage {
  items: { id: string; name: string; coverUrl: string | null }[];
  remainingIds: string[];
  essential?: { name: string; coverUrl: string | null } | null;
}
export interface PlaylistEssentialCategory {
  name: string;
  displayName: string | null;
}
export interface PlaylistEssentialPage {
  items: PlaylistSquareCard[];
  continuation: string | null;
}
export interface PlaylistTagGroup {
  id: number;
  name: string;
  tags: { name: string; displayName: string | null }[];
}
export interface PlaylistTopTags {
  recommendationEnabled: boolean;
  tags: string[];
}
export interface PlaylistSquareCard {
  id: string;
  name: string;
  coverUrl: string | null;
  playCount: number | null;
}
export interface PlaylistSquarePage {
  recommendationEnabled: boolean;
  groups: { title: string | null; items: PlaylistSquareCard[] }[];
  continuation: string | null;
}

export interface PlaylistSubscribersPage {
  subscribers: {
    userId: string | null;
    nickname: string;
    avatarUrl: string | null;
    signature: string;
  }[];
  previousOffset: number | null;
  nextOffset: number | null;
}

export interface PlaylistView {
  subscribedCount?: number | null;
  playCount?: number | null;
  id: string;
  name: string;
  coverUrl: string | null;
  description: string;
  creator: string;
  creatorAvatarUrl?: string | null;
  creatorId?: string | null;
  tags?: string[];
  headerDate?: { kind: 'created' | 'updated'; timestampMs: number } | null;
  trackCount: number;
  owned: boolean;
  favorite: boolean;
  private: boolean;
}
export interface ArtistReference {
  id: string | null;
  name: string;
}
export interface TrackView {
  playlistSongCommentMenu?: SongRowCommentMenu;
  voiceId?: string;
  id: string;
  name: string;
  artists: string[];
  artistRefs?: ArtistReference[];
  album: string;
  albumId?: string | null;
  coverUrl: string | null;
  durationMs: number;
  albumPopularity?: number;
  fee: number;
  missing: boolean;
  local: boolean;
}
export interface TrackPage {
  revision: string | null;
  tracks: TrackView[];
  total: number;
  offset: number;
  more: boolean;
  playlist: PlaylistView | null;
  playlistLocal?: {
    revision: string;
    hasMembers: boolean;
    hasUnboundLegacy?: boolean;
    rows: Record<string, string>;
  };
  playlistOffline?: { cachedAtMs: number; downloadIds?: string[] };
  daily?: { cached: boolean; showFreeTag: boolean; birthdayToday?: boolean };
  styleTags?: StyleCategory;
}
export type QrStatus =
  | 'idle'
  | 'creating'
  | 'waiting_scan'
  | 'waiting_confirmation'
  | 'authorized'
  | 'expired'
  | 'challenge'
  | 'cancelled'
  | 'error';
export interface QrView {
  pollAfterMs: number;
  operationId: string;
  status: QrStatus;
  modules: boolean[][];
}
export interface LikeResult {
  id: string;
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'rejected' | 'unchanged';
  observed: boolean | null;
}
export interface CoreFailure {
  code: string;
  message: string;
  retryable: boolean;
}
export function failure(error: unknown): CoreFailure {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    'message' in error &&
    typeof error.code === 'string' &&
    typeof error.message === 'string'
  ) {
    return {
      code: error.code,
      message: error.message,
      retryable: 'retryable' in error && error.retryable === true,
    };
  }
  return { code: 'connection', message: '无法连接音乐服务，请重新打开客户端', retryable: true };
}
