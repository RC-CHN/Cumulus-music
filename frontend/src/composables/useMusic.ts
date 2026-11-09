import type { AccountDetailKind } from '../services/accountDetail';
import { computed, ref, shallowRef } from 'vue';
import type {
  AccountData,
  AccountView,
  CoreClient,
  LikeResult,
  PlaylistView,
  StyleSeedHandle,
  TrackPage,
  TrackView,
} from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import type { CommentResource } from '../services/comments';
import type { DownloadTab } from '../services/downloads';
import type { QueueLikeTarget } from '../services/player';
import type { VoiceSongRowTarget } from '../services/voiceSongs';
export type PlaylistSection = 'songs' | 'comments' | 'subscribers';
type Destination =
  | { kind: 'decoration' }
  | { kind: 'settings'; accountDetail?: AccountDetailKind }
  | { kind: 'listened_books' }
  | { kind: 'history'; scope: import('../services/history').HistoryKind }
  | {
      kind: 'collections';
      scope: import('../services/collections').CollectionScope;
      collectionVisit?: number;
    }
  | ({ kind: 'catalog' } & import('../services/catalogNavigation').CatalogLocation)
  | { kind: 'artist_gallery' }
  | { kind: 'discovery_home' }
  | { kind: 'ordinary_essential' }
  | ({ kind: 'vertical_zone' } & import('../services/verticalZone').VerticalZoneLocation)
  | { kind: 'follow_new_song' }
  | {
      kind: 'latest_releases';
      tab: import('../services/latestReleases').LatestReleaseTab;
      latestVisit?: number;
    }
  | { kind: 'community' }
  | { kind: 'community_detail'; handle: string }
  | { kind: 'community_topic'; name: string; visit: number }
  | { kind: 'scene_fm' }
  | { kind: 'member' }
  | { kind: 'fm' }
  | {
      kind: 'rankings';
      query: import('../services/rankings').RankingQuery;
      rankingState?: import('../services/rankings').RankingUiState;
    }
  | { kind: 'together' }
  | { kind: 'podcasts'; messageRadioVisit?: number }
  | { kind: 'podcast_category'; id: string }
  | { kind: 'podcast_discovery' }
  | {
      kind: 'podcast_rankings';
      target?: import('../services/podcast').PodcastRankTarget;
      rankVisit?: number;
    }
  | { kind: 'local' }
  | { kind: 'local_batch'; query: import('../services/local').LocalQuery }
  | { kind: 'daily' }
  | { kind: 'daily_batch' }
  | { kind: 'style_recommendations'; seed?: StyleSeedHandle; origin?: 'home_recent' }
  | { kind: 'library' }
  | {
      kind: 'downloads';
      downloadVisit?: number;
      initialTab?: DownloadTab;
      initialCategory?: 'song' | 'voice';
    }
  | { kind: 'cloud' }
  | { kind: 'mvs' }
  | { kind: 'mv'; id: string; taskId?: string }
  | {
      kind: 'comments';
      resource: CommentResource;
      title: string;
      initialCommentId?: string;
      initialSort?: 'hot';
    }
  | {
      kind: 'playlist';
      id: string;
      section?: PlaylistSection;
      subscriberReturn?: number;
      initialCommentId?: string;
      zonePlaylistReceipt?: string;
    }
  | { kind: 'playlist_tag'; tag: string }
  | { kind: 'playlist_essential'; tag: string }
  | {
      kind: 'search_reason';
      label: string;
      target: import('../services/search').SearchReasonTarget;
    }
  | { kind: 'playlist_creator' | 'profile_notes' | 'profile_podcasts'; id: string }
  | ({
      kind: 'profile_play_record';
    } & import('../services/profilePlayRecord').ProfilePlayRecordTarget)
  | ({ kind: 'profile_relations' } & import('../services/profileRelations').ProfileRelationTarget)
  | {
      kind: 'search';
      keyword: string;
      displayKeyword?: string;
      defaultReceipt?: string;
      correctionReceipt?: string;
      category?: import('../services/search').SearchCategory;
      channel?: import('../services/search').SearchChannel;
      pagination?: import('../services/searchPagination').SearchPageStep;
      correct?: boolean;
      rewrittenKeyword?: string;
      searchId?: number;
    };

export type Page = Destination & { choice?: true };

export function useMusic(
  core: CoreClient = useCore(),
  beforeNavigate: (commit: () => void) => void = (commit) => commit(),
) {
  const account = ref<AccountView>({
    epoch: '0',
    status: 'unknown',
    nickname: null,
    avatarUrl: null,
  });
  const playlists = shallowRef<PlaylistView[]>([]);
  const liked = shallowRef(new Set<string>());
  const busyLikes = shallowRef(new Set<string>());
  const likeActivity = ref(0);
  const page = shallowRef<Page>({ kind: 'daily' });
  const result = shallowRef<TrackPage>();
  const loading = ref(false);
  const initializing = ref(true);
  const accountReady = ref(false);
  const error = ref('');
  const notice = ref('');
  const likeNotice = ref('');
  const libraryError = ref('');
  const playlistError = ref('');
  const resourcesLoading = ref(false);
  const history: Page[] = [];
  const historyLength = ref(0);
  const playlistPrefixes = new Map<Page, TrackPage>();
  const styleBackRevisions = new Map<Page, string>();
  let request = 0;
  let searchId = 0;
  let downloadVisit = 0;
  let catalogVisit = 0;
  let podcastRankVisit = 0;
  let collectionVisit = 0;
  let latestVisit = 0;
  let accountRequest = 0;
  let resourceRequest = 0;
  let likeRevision = 0;
  const signedIn = computed(() => account.value.status === 'authenticated');
  function accept<T>(data: AccountData<T>): T | undefined {
    return data.epoch === account.value.epoch && signedIn.value ? data.value : undefined;
  }
  function handle(e: unknown) {
    const issue = failure(e);
    error.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
      request++;
      loading.value = false;
      resourceRequest++;
      resourcesLoading.value = false;
      account.value = { ...account.value, status: 'expired', nickname: null, avatarUrl: null };
      notice.value = '';
      playlists.value = [];
      liked.value = new Set();
      result.value = undefined;
    }
  }
  function handleLike(e: unknown) {
    const issue = failure(e);
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') handle(e);
    else likeNotice.value = issue.message;
  }
  async function resources() {
    const current = ++resourceRequest;
    const owner = account.value.epoch;
    const likesAtStart = likeRevision;
    libraryError.value = '';
    resourcesLoading.value = true;
    const replies = await Promise.allSettled([core.playlists(), core.likedIds()]);
    if (current !== resourceRequest || owner !== account.value.epoch) return;
    resourcesLoading.value = false;
    const lists = replies[0];
    if (lists.status === 'fulfilled') {
      const v = accept(lists.value);
      if (v) {
        playlists.value = v;
        playlistError.value = '';
      }
    } else {
      libraryError.value = failure(lists.reason).message;
      playlistError.value = libraryError.value;
      handle(lists.reason);
    }
    const likes = replies[1];
    if (likes.status === 'fulfilled') {
      const v = accept(likes.value);
      if (v && likesAtStart === likeRevision) {
        liked.value = new Set(v);
        likeNotice.value = '';
      }
    } else {
      libraryError.value = failure(likes.reason).message;
      handle(likes.reason);
    }
  }
  function mergeVerifiedPlaylistLikes(epoch: string, ids: readonly string[]) {
    if (epoch !== account.value.epoch || !signedIn.value) return;
    const confirmed = ids.filter((id) => !busyLikes.value.has(id));
    if (!confirmed.length) return;
    likeRevision++;
    const next = new Set([...liked.value, ...confirmed]);
    if (next.size !== liked.value.size) liked.value = next;
  }
  async function refreshLikes() {
    const revision = ++likeRevision;
    const epoch = account.value.epoch;
    try {
      const response = await core.likedIds();
      const value = accept(response);
      if (value && revision === likeRevision && account.value.epoch === epoch) {
        liked.value = new Set(value);
        likeNotice.value = '';
      }
    } catch (error) {
      if (revision === likeRevision && account.value.epoch === epoch) handleLike(error);
    }
  }
  async function load(
    more = false,
    retainDailyCache = false,
    prefix?: TrackPage,
    styleBack?: string,
  ) {
    const current = ++request;
    error.value = '';
    loading.value = true;
    const target = page.value;
    const keepingCache =
      retainDailyCache && signedIn.value && target.kind === 'daily' && result.value?.daily?.cached;
    const offset = more ? (result.value?.tracks.length ?? 0) : 0;
    if (!more && !keepingCache) result.value = undefined;
    try {
      if (
        !signedIn.value ||
        target.kind === 'search' ||
        target.kind === 'search_reason' ||
        target.kind === 'library' ||
        target.kind === 'playlist_tag' ||
        target.kind === 'playlist_essential' ||
        target.kind === 'playlist_creator' ||
        target.kind === 'profile_notes' ||
        target.kind === 'profile_podcasts' ||
        target.kind === 'profile_play_record' ||
        target.kind === 'profile_relations' ||
        target.kind === 'history' ||
        target.kind === 'settings' ||
        target.kind === 'decoration' ||
        target.kind === 'local' ||
        target.kind === 'local_batch' ||
        target.kind === 'downloads' ||
        target.kind === 'cloud' ||
        target.kind === 'mvs' ||
        target.kind === 'podcasts' ||
        target.kind === 'listened_books' ||
        target.kind === 'podcast_category' ||
        target.kind === 'podcast_discovery' ||
        target.kind === 'podcast_rankings' ||
        target.kind === 'together' ||
        target.kind === 'rankings' ||
        target.kind === 'fm' ||
        target.kind === 'scene_fm' ||
        target.kind === 'member' ||
        target.kind === 'catalog' ||
        target.kind === 'artist_gallery' ||
        target.kind === 'discovery_home' ||
        target.kind === 'ordinary_essential' ||
        target.kind === 'vertical_zone' ||
        target.kind === 'follow_new_song' ||
        target.kind === 'latest_releases' ||
        target.kind === 'community' ||
        target.kind === 'community_detail' ||
        target.kind === 'community_topic' ||
        target.kind === 'collections' ||
        target.kind === 'mv' ||
        target.kind === 'comments'
      )
        return;
      const restored =
        target.kind === 'style_recommendations' && styleBack
          ? await core.restoreStyleRecommendations(
              account.value.epoch,
              styleBack,
              target.seed ?? null,
            )
          : null;
      // Do not request a new style list after this Back navigation was replaced.
      if (current !== request) return;
      const playlistPage = { offset, limit: 100, revision: more ? result.value?.revision : null };
      const data =
        restored ??
        (target.kind === 'daily' || target.kind === 'daily_batch'
          ? await core.dailySongs()
          : target.kind === 'style_recommendations'
            ? target.seed
              ? await core.seededStyleRecommendations(account.value.epoch, target.seed)
              : await core.styleRecommendations(account.value.epoch)
            : target.kind === 'playlist' && target.zonePlaylistReceipt
              ? await core.verticalZonePlaylistDetail(
                  account.value.epoch,
                  target.zonePlaylistReceipt,
                  target.id,
                  playlistPage,
                )
              : await core.playlistPage(target.id, playlistPage));
      if (current !== request) return;
      const value = accept(data);
      if (!value) {
        if (keepingCache) result.value = undefined;
        return;
      }
      result.value =
        more && result.value
          ? { ...value, tracks: [...result.value.tracks, ...value.tracks] }
          : prefix?.revision &&
              prefix.revision === value.revision &&
              prefix.tracks.length > value.tracks.length
            ? {
                ...value,
                tracks: [...value.tracks, ...prefix.tracks.slice(value.tracks.length)],
                more: prefix.more,
              }
            : value;
    } catch (e) {
      if (current === request) {
        if (keepingCache) result.value = undefined;
        handle(e);
      }
    } finally {
      if (current === request) loading.value = false;
    }
  }
  function retryDailyCache() {
    if (!loading.value && page.value.kind === 'daily' && result.value?.daily?.cached)
      return load(false, true);
  }
  async function primeLocalAccount() {
    const current = accountRequest;
    try {
      const next = await core.accountSnapshot();
      if (current !== accountRequest || accountReady.value) return;
      account.value = next;
      accountReady.value = true;
    } catch {
      // The library itself does not need an epoch; online bootstrap can
      // still provide one for playback if this local snapshot failed.
    }
  }
  async function bootstrap(restoring = true) {
    const current = ++accountRequest;
    request++;
    resourceRequest++;
    resourcesLoading.value = false;
    initializing.value = true;
    error.value = '';
    try {
      const next = restoring ? await core.restoreAccount() : await core.accountSnapshot();
      if (current !== accountRequest) return;
      if (next.epoch !== account.value.epoch) {
        // Account-owned history expires, but Back from a device-local
        // batch must still return to its ordinary local list.
        const previous = history.at(-1);
        history.length = 0;
        if (page.value.kind === 'local_batch' && previous?.kind === 'local') history.push(previous);
        playlistPrefixes.clear();
        styleBackRevisions.clear();
        historyLength.value = history.length;
        if (
          [
            'playlist',
            'playlist_tag',
            'playlist_essential',
            'playlist_creator',
            'profile_notes',
            'community_topic',
            'profile_podcasts',
            'listened_books',
            'profile_play_record',
            'profile_relations',
            'comments',
            'catalog',
            'search',
            'daily_batch',
            'mv',
          ].includes(page.value.kind) ||
          (page.value.kind === 'style_recommendations' && page.value.seed)
        )
          page.value = { kind: 'daily' };
        playlistError.value = '';
        libraryError.value = '';
        playlists.value = [];
        liked.value = new Set();
        result.value = undefined;
        busyLikes.value = new Set();
        likeNotice.value = '';
        notice.value = '';
      }
      account.value = next;
      accountReady.value = true;
      if (signedIn.value) {
        await resources();
        if (current !== accountRequest) return;
        await load();
      }
    } catch (e) {
      if (current === accountRequest) handle(e);
    } finally {
      if (current === accountRequest) initializing.value = false;
    }
  }
  function navigate(next: Page, subscriberReturn?: number) {
    const local = next.kind === 'local' || next.kind === 'local_batch';
    if (initializing.value && !local) return;
    navigation(() => {
      const keepDaily =
        page.value.kind === 'daily' &&
        next.kind === 'daily_batch' &&
        !!result.value?.daily &&
        !loading.value;
      const from =
        page.value.kind === 'playlist' && subscriberReturn !== undefined
          ? { ...page.value, subscriberReturn }
          : page.value;
      const shown = result.value;
      if (
        from.kind === 'playlist' &&
        shown?.playlist?.id === from.id &&
        shown.tracks.length > 100
      ) {
        playlistPrefixes.set(from, shown);
        if (playlistPrefixes.size > 20)
          playlistPrefixes.delete(playlistPrefixes.keys().next().value!);
      }
      if (
        from.kind === 'style_recommendations' &&
        !loading.value &&
        shown?.revision?.startsWith(from.seed ? 'seeded-style-' : 'style-')
      ) {
        styleBackRevisions.set(from, shown.revision);
        if (styleBackRevisions.size > 20)
          styleBackRevisions.delete(styleBackRevisions.keys().next().value!);
      }
      history.push(from);
      historyLength.value = history.length;
      page.value =
        next.kind === 'search' && next.searchId === undefined
          ? { ...next, searchId: ++searchId }
          : next.kind === 'downloads'
            ? { ...next, downloadVisit: ++downloadVisit }
            : next.kind === 'podcast_rankings' && next.rankVisit === undefined
              ? { ...next, rankVisit: ++podcastRankVisit }
              : next.kind === 'catalog' && next.catalogVisit === undefined
                ? { ...next, catalogVisit: ++catalogVisit }
                : next.kind === 'collections' && next.collectionVisit === undefined
                  ? { ...next, collectionVisit: ++collectionVisit }
                  : next.kind === 'latest_releases' && next.latestVisit === undefined
                    ? { ...next, latestVisit: ++latestVisit }
                    : next;
      if (!keepDaily) void load();
    }, local);
  }
  function back() {
    navigation(
      () => {
        const previous = history.pop();
        historyLength.value = history.length;
        if (previous) {
          page.value = previous;
          const prefix = playlistPrefixes.get(previous);
          const styleBack = styleBackRevisions.get(previous);
          playlistPrefixes.delete(previous);
          styleBackRevisions.delete(previous);
          if (!initializing.value || previous.kind === 'local' || previous.kind === 'local_batch')
            void load(false, false, prefix, styleBack);
        }
      },
      page.value.kind === 'local' || page.value.kind === 'local_batch',
    );
  }
  function replacePlaylistSection(section: PlaylistSection) {
    const current = page.value;
    if (current.kind !== 'playlist' || (current.section ?? 'songs') === section) return;
    // The parent detail and its data survive a child route replacement.
    page.value = {
      kind: 'playlist',
      id: current.id,
      section,
      ...(current.zonePlaylistReceipt ? { zonePlaylistReceipt: current.zonePlaylistReceipt } : {}),
    };
  }
  function replace(next: Page) {
    if (initializing.value) return;
    navigation(() => {
      page.value =
        next.kind === 'downloads'
          ? { ...next, downloadVisit: ++downloadVisit }
          : next.kind === 'catalog' && next.catalogVisit === undefined
            ? { ...next, catalogVisit: ++catalogVisit }
            : next.kind === 'collections' && next.collectionVisit === undefined
              ? {
                  ...next,
                  collectionVisit:
                    page.value.kind === 'collections'
                      ? (page.value.collectionVisit ?? ++collectionVisit)
                      : ++collectionVisit,
                }
              : next;
      void load();
    });
  }
  function navigation(commit: () => void, local = false) {
    const from = page.value,
      owner = account.value.epoch;
    let used = false;
    beforeNavigate(() => {
      if (
        used ||
        (initializing.value && !local) ||
        page.value !== from ||
        account.value.epoch !== owner
      )
        return;
      used = true;
      commit();
    });
  }
  async function changeLike(
    id: string,
    submit: (epoch: string, desired: boolean) => Promise<AccountData<LikeResult>>,
    announce = false,
  ) {
    if (!signedIn.value || busyLikes.value.has(id)) return;
    const owner = account.value.epoch;
    const active = () => signedIn.value && account.value.epoch === owner;
    busyLikes.value = new Set(busyLikes.value).add(id);
    likeNotice.value = '';
    likeRevision++;
    try {
      const response = await submit(owner, !liked.value.has(id));
      const value = accept(response);
      if (!value || value.id !== id) return;
      if (value.observed !== null) {
        const next = new Set(liked.value);
        value.observed ? next.add(id) : next.delete(id);
        liked.value = next;
      }
      if (value.phase !== 'confirmed' && value.phase !== 'unchanged')
        likeNotice.value =
          value.phase === 'rejected'
            ? '服务未确认此次操作，请刷新后核对'
            : '操作结果尚未确认，请刷新核对后再操作';
      else if (announce && value.observed !== null)
        likeNotice.value = value.observed ? '已添加到我喜欢的音乐' : '已取消喜欢';
    } catch (e) {
      if (active()) handleLike(e);
    } finally {
      if (active()) {
        likeRevision++;
        const next = new Set(busyLikes.value);
        next.delete(id);
        busyLikes.value = next;
        likeActivity.value++;
      }
    }
  }
  function like(track: Pick<TrackView, 'id'>) {
    return changeLike(track.id, (owner, desired) => core.setLiked(owner, track.id, desired));
  }
  function likeFromQueue(target: QueueLikeTarget) {
    if (target.epoch !== account.value.epoch) return;
    return changeLike(
      target.songId,
      (owner, desired) => core.playerQueueLike(owner, target.revision, target.entryId, desired),
      true,
    );
  }
  function likeFromVoiceSongs(target: VoiceSongRowTarget, songId: string) {
    if (target.epoch !== account.value.epoch) return;
    return changeLike(
      songId,
      (owner, desired) =>
        core.playerVoiceSongLike(owner, target.playId, target.receipt, target.index, desired),
      true,
    );
  }
  return {
    replace,
    replacePlaylistSection,
    account,
    playlists,
    liked,
    busyLikes,
    likeActivity,
    page,
    result,
    loading,
    initializing,
    accountReady,
    primeLocalAccount,
    error,
    notice,
    likeNotice,
    libraryError,
    playlistError,
    resourcesLoading,
    historyLength,
    signedIn,
    resources,
    refreshLikes,
    mergeVerifiedPlaylistLikes,
    load,
    retryDailyCache,
    bootstrap,
    navigate: (next: Page) => navigate(next),
    openSubscriberProfile: (id: string, token: number) =>
      navigate({ kind: 'playlist_creator', id }, token),
    back,
    like,
    likeFromQueue,
    likeFromVoiceSongs,
  };
}
