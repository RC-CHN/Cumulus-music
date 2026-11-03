<script setup lang="ts">
import SearchBox from './components/search/SearchBox.vue';
import { useMotion } from './composables/useMotion';
import { createPageTransition } from './services/pageTransition';
import SearchResultsPage from './components/search/SearchResultsPage.vue';
import SearchReasonPage from './components/search/SearchReasonPage.vue';
import { createSearchViewportMemory } from './services/searchViewport';
import { createCollectionViewportMemory } from './services/collectionViewport';
import ContentTooltip from './components/ContentTooltip.vue';
import type { SearchItem } from './services/search';
import {
  searchCardIsCurrent,
  searchCardSelection,
  searchMvPlaybackTarget,
  searchVoiceSelection,
  type SearchQueueRequest,
  searchCurrentSongId,
  searchCurrentVoiceId,
} from './services/searchCards';
import { catalogResourceSelection, type AlbumCardRequest } from './services/catalogCards';
import AlbumActions from './components/catalog/AlbumActions.vue';
import ArtistActions from './components/catalog/ArtistActions.vue';
import type { ArtistCardRequest } from './services/artistCards';
import { computed, onMounted, onUnmounted, watch, ref, shallowRef, nextTick } from 'vue';
import { onDesktopLyricOpen } from './adapters/desktopLyrics';
import { onMiniReturn } from './adapters/mini';
import { createLyricReportClient, onLyricReportOpen } from './adapters/lyricReports';
import { useLyricReport } from './composables/useLyricReport';
import LyricReportDialog from './components/LyricReportDialog.vue';
import { useMiniFmFocus } from './composables/useMiniFmFocus';
import { useLocalShortcuts } from './composables/useLocalShortcuts';
import Icon from './components/Icon.vue';
import WindowControls from './components/WindowControls.vue';
import WindowClosePrompt from './components/WindowClosePrompt.vue';
import AppearanceMenu from './components/AppearanceMenu.vue';
import PlaylistLibraryPage from './components/playlist/PlaylistLibraryPage.vue';
import AppSidebar from './components/sidebar/AppSidebar.vue';
import { libraryView } from './services/playlistLibrary';
import DailyHeading from './components/DailyHeading.vue';
import DailyBatchPage from './components/DailyBatchPage.vue';
import ActionMenu from './components/ActionMenu.vue';
import StyleTagBrowser from './components/StyleTagBrowser.vue';
import StyleHomeBrowser from './components/StyleHomeBrowser.vue';
import PlaylistHero from './components/playlist/PlaylistHero.vue';
import PlaylistTagBrowse from './components/playlist/PlaylistTagBrowse.vue';
import PlaylistCreatorPage from './components/playlist/PlaylistCreatorPage.vue';
import { usePlaylistCreator } from './composables/usePlaylistCreator';
import { usePlaylistTagBrowse } from './composables/usePlaylistTagBrowse';
import PlaylistSubscribers from './components/playlist/PlaylistSubscribers.vue';
import { filterTrackScope } from './services/trackScopeSearch';
import TrackScopeSearch from './components/TrackScopeSearch.vue';
import { usePlaylistSubscribers } from './composables/usePlaylistSubscribers';
import { usePlaylistCommentTab } from './composables/usePlaylistCommentTab';
import PlaylistEditorPage from './components/playlist/PlaylistEditorPage.vue';
import { playlistMenuItems, type PlaylistMenuScene } from './services/playlistMenu';
import PlaylistActions from './components/playlist/PlaylistActions.vue';
import PlaylistBatchPage from './components/playlist/PlaylistBatchPage.vue';
import TrackActions from './components/TrackActions.vue';
import ModalTransition from './components/ModalTransition.vue';
import { useModalOrigin } from './composables/useModalOrigin';
import LoginDialog from './components/LoginDialog.vue';
import AccountDialog from './components/AccountDialog.vue';
import AccountWelcome from './components/AccountWelcome.vue';
import TrackTable from './components/TrackTable.vue';
import CatalogPage from './components/CatalogPage.vue';
import { useCatalogWorkspace } from './composables/useCatalogWorkspace';
import type { CatalogSession } from './composables/useCatalogSession';
import CollectionsPage from './components/CollectionsPage.vue';
import HistoryPage from './components/HistoryPage.vue';
import SettingsPage from './components/SettingsPage.vue';
import { useSettings } from './composables/useSettings';
import { useRelay } from './composables/useRelay';
import { useRelayPublication } from './composables/useRelayPublication';
import RelayPrompt from './components/RelayPrompt.vue';
import LikeRecoveryDialog from './components/LikeRecoveryDialog.vue';
import LikeRecoveryNotice from './components/LikeRecoveryNotice.vue';
import { useLikeRecovery } from './composables/useLikeRecovery';
import { useHistory } from './composables/useHistory';
import type { HistoryKind, HistoryResource } from './services/history';
import type { AlbumCard, CatalogResource, CatalogSelectionPage } from './services/catalog';
import PlayerBar from './components/PlayerBar.vue';
import { useIntelligence } from './composables/useIntelligence';
import QueuePanel from './components/QueuePanel.vue';
import type { TrackMenuRequest, TrackPlayGesture } from './services/trackMenu';
import {
  currentCatalogDownload,
  type QueueCollectionView,
  type SearchResultPlayback,
  type SongRowDoubleClickSelection,
} from './services/player';
import LyricsPanel from './components/LyricsPanel.vue';
import PlaylistDialog from './components/PlaylistDialog.vue';
import PlaylistCollectDialog from './components/PlaylistCollectDialog.vue';
import DownloadPage from './components/DownloadPage.vue';
import ColdOfflinePage from './components/download/ColdOfflinePage.vue';
import { useColdOffline } from './composables/useColdOffline';
import DownloadBatchPage from './components/download/DownloadBatchPage.vue';
import type { BatchCategory } from './services/downloadBatch';
import type { DownloadQuery } from './services/downloads';
import DownloadDialog from './components/DownloadDialog.vue';
import CloudPage from './components/CloudPage.vue';
import CloudBatchPage from './components/cloud/CloudBatchPage.vue';
import MvCollection from './components/MvCollection.vue';
import MvPlayer from './components/MvPlayer.vue';
import CommentsPage from './components/CommentsPage.vue';
import {
  commentResourceSelection,
  currentCommentResource,
  type CommentResourceCard,
  type CommentResourceDestination,
} from './services/commentResource';
import LocalPage from './components/LocalPage.vue';
import PodcastPage from './components/PodcastPage.vue';
import TogetherPage from './components/TogetherPage.vue';
import RankingsPage from './components/RankingsPage.vue';
import FmPage from './components/FmPage.vue';
import SceneFmPage from './components/SceneFmPage.vue';
import MemberPage from './components/MemberPage.vue';
import { useMember } from './composables/useMember';
import type { MemberTarget } from './services/member';
import { useSceneFm } from './composables/useSceneFm';
import { useFm } from './composables/useFm';
import { useRankings } from './composables/useRankings';
import { rankingQueryKey, type RankingQuery } from './services/rankings';
import { useTogether } from './composables/useTogether';
import { usePodcasts } from './composables/usePodcasts';
import type { PodcastScope } from './services/podcast';
import {
  failure,
  type PlaylistView,
  type StyleSeedHandle,
  type TrackView,
} from './services/contracts';
import { useLocal } from './composables/useLocal';
import { useLocalMatching } from './composables/useLocalMatching';
import type { LocalQuery } from './services/local';
import { useComments } from './composables/useComments';
import type { CommentResource } from './services/comments';
import { useMv } from './composables/useMv';
import { useCloud } from './composables/useCloud';
import { useDownloads } from './composables/useDownloads';
import type { DownloadDialogTarget } from './services/downloads';
import { usePlaylists } from './composables/usePlaylists';
import { playlistSaved, type PlaylistDialogTarget } from './services/playlists';
import { usePlayer } from './composables/usePlayer';
import { createPlayerResume } from './services/playerResume';
import { useMusic } from './composables/useMusic';
import { downloadSectionQuery } from './services/downloadPage';
import { useCore } from './services/core';
const core = useCore();
const searchMemory = createSearchViewportMemory();
const collectionMemory = createCollectionViewportMemory();
const collectionRevision = ref(0);
const albumActions = ref<InstanceType<typeof AlbumActions>>();
const artistActions = ref<InstanceType<typeof ArtistActions>>();
const collectionsPageRef = ref<InstanceType<typeof CollectionsPage>>();
const playlistLibraryState = ref(libraryView());
const settingsPage = ref<InstanceType<typeof SettingsPage>>();
const music = useMusic(undefined, (commit) => {
  const proceed = () => {
    catalogDetailPage.value?.checkpoint();
    collectionsPageRef.value?.checkpoint();
    commit();
  };
  if (batchTarget.value && batchPage.value)
    batchPage.value.requestLeave(() => {
      void closeBatch(false);
      proceed();
    });
  else if (downloadBatch.value && downloadBatchPage.value)
    downloadBatchPage.value.requestLeave(() => {
      closeDownloadBatch(false);
      proceed();
    });
  else if (cloudBatchTarget.value && cloudBatchPage.value)
    cloudBatchPage.value.requestLeave(() => {
      void closeCloudBatch(false);
      proceed();
    });
  else if (playlistEditor.value && playlistEditorPage.value)
    playlistEditorPage.value.requestLeave(() => {
      closePlaylistEditor(false);
      proceed();
    });
  else if (settingsPage.value) settingsPage.value.requestLeave(proceed);
  else proceed();
});
const catalogWorkspace = useCatalogWorkspace(
  core,
  () => {
    void music.bootstrap(false);
  },
  8,
  catalogMembershipChanged,
);
const catalogSession = shallowRef<CatalogSession>();
watch(
  [() => music.page.value, () => music.account.value.epoch, () => music.account.value.status],
  () => {
    catalogSession.value = catalogWorkspace.open(
      music.account.value,
      music.page.value.kind === 'catalog' ? music.page.value : undefined,
    );
  },
  { immediate: true, flush: 'sync' },
);
const settings = useSettings(core, () => {
  void music.bootstrap(false);
});
watch(
  () => [music.account.value.epoch, music.account.value.status],
  () => {
    searchMemory.connect(music.account.value);
    collectionMemory.connect(music.account.value);
  },
  { immediate: true },
);
const relay = useRelay(core, () => {
  void music.bootstrap(false);
});
const publication = useRelayPublication(core, () => {
  void music.bootstrap(false);
});
const recent = useHistory(core, () => {
  void music.bootstrap(false);
});
function playHistory(kind: HistoryKind, revision: string, ids: string[], index: number) {
  void player.select({
    kind: 'history',
    history_kind: kind,
    revision,
    ids,
    index,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
function openHistory(resource: HistoryResource) {
  switch (resource.kind) {
    case 'song':
      if (resource.track.local) music.navigate({ kind: 'local' });
      else playlistDialog.value = { kind: 'track', track: resource.track };
      break;
    case 'album':
      openCatalog({ kind: 'album', id: resource.album.id });
      break;
    case 'playlist':
      music.navigate({ kind: 'playlist', id: resource.playlist.id });
      break;
    case 'voice':
      if (resource.voice.radioId) openSceneRadio(resource.voice.radioId);
      else openComments({ kind: 'voice', id: resource.voice.id }, resource.voice.name);
      break;
    default:
      openSceneRadio(resource.radio.id);
  }
}
const member = useMember(core, () => {
  void music.bootstrap(false);
});
function openMemberTarget(target: MemberTarget) {
  if (target.kind === 'external') void member.openExternal(target.key);
  else if (target.kind === 'playlist') music.navigate({ kind: 'playlist', id: target.id });
  else if (target.kind === 'album') openRanking({ kind: 'album', id: target.id });
  else if (target.kind === 'radio') openSceneRadio(target.id);
  else
    void player.select({
      kind: 'songs',
      ids: [target.id],
      index: 0,
      quality: player.view.value?.qualityPreferences.quality ?? 'standard',
    });
}
function playMember(section: string | null, index: number) {
  if (!member.view.value) return;
  const shown = member.view.value.page;
  void player.select({
    kind: 'member',
    query: shown.kind === 'history' ? { kind: 'history', date: shown.date } : { kind: 'home' },
    revision: member.view.value.revision,
    section,
    index,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
function playMemberPlaylist(id: string, startId: string | null) {
  void player.select({
    kind: 'playlist',
    id,
    start_id: startId,
    revision: null,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
const sceneFm = useSceneFm(core, () => {
  void music.bootstrap(false);
});
function openSceneRadio(id: string) {
  podcasts.navigate({ kind: 'radio', id, ascending: true });
  music.navigate({ kind: 'podcasts' });
}
function openSearchChartRadio(id: string) {
  const scope = podcasts.query.value.scope;
  if (page.value.kind === 'podcasts' && scope.kind === 'radio' && scope.id === id) return;
  openSceneRadio(id);
}
const fm = useFm(undefined, () => {
  void music.bootstrap(false);
});
const rankings = useRankings(undefined, () => {
  void music.bootstrap(false);
});
const rankingsPageRef = ref<InstanceType<typeof RankingsPage>>();
function checkpointRanking(target: {
  query: RankingQuery;
  rankingState?: import('./services/rankings').RankingUiState;
}) {
  const saved = rankingsPageRef.value?.checkpoint();
  if (saved?.queryKey === rankingQueryKey(target.query)) target.rankingState = saved;
}
function openRanking(
  query: RankingQuery,
  rankingState?: import('./services/rankings').RankingUiState,
) {
  if (query.kind === 'album') openCatalog({ kind: 'album', id: query.id });
  else music.navigate({ kind: 'rankings', query, rankingState });
}
function openCatalog(resource: CatalogResource) {
  music.navigate({ kind: 'catalog', resource });
}
function openPlaybackSource(
  target: import('./services/playbackMetadata').PlaybackSourceDestination,
) {
  player.lyricsOpen.value = false;
  switch (target.kind) {
    case 'playlist':
      music.navigate({ kind: 'playlist', id: target.id });
      break;
    case 'album':
    case 'artist':
      openCatalog({ kind: target.kind, id: target.id });
      break;
    case 'podcast':
      openSceneRadio(target.id);
      break;
    case 'local':
    case 'cloud':
    case 'downloads':
      music.navigate({ kind: target.kind });
      break;
  }
}
const searchBox = ref<InstanceType<typeof SearchBox>>();
function searchOriginal(keyword: string) {
  if (
    page.value.kind !== 'search' ||
    page.value.correct === false ||
    page.value.keyword !== keyword
  )
    return;
  if (searchBox.value?.commitOriginal(keyword) !== keyword) return;
  music.navigate({ kind: 'search', keyword, correct: false });
}
function openSearchItem(item: SearchItem) {
  if (item.kind === 'artist') openCatalog({ kind: 'artist', id: item.artist.id });
  else if (item.kind === 'album') openCatalog({ kind: 'album', id: item.album.id });
  else if (item.kind === 'playlist') music.navigate({ kind: 'playlist', id: item.playlist.id });
  else if (item.kind === 'user' && !item.user.unavailable)
    music.navigate({ kind: 'playlist_creator', id: item.user.id });
  else if (item.kind === 'podcast') openSceneRadio(item.podcast.id);
  else if (item.kind === 'voice') {
    if (item.voice.radioId) openSceneRadio(item.voice.radioId);
    else music.notice.value = '这条声音暂未提供所属播客';
  } else if (item.kind === 'mv') {
    const id = searchMvPlaybackTarget(item);
    if (id) openMv(id);
    else music.notice.value = '此视频暂不可打开';
  }
}
function playSearchChart(keywords: string[]) {
  if (account.value.status !== 'authenticated' || !player.ready.value || player.busy.value) return;
  const quality = player.view.value?.qualityPreferences.quality ?? 'standard';
  void player.prepareSelection(async (epoch) => {
    const response = await core.searchDiscovery(epoch, { kind: 'chart_songs', keywords });
    if (response.epoch !== epoch || response.value.kind !== 'chart_songs')
      throw { code: 'stale', message: '热搜榜播放已失效', retryable: false };
    return { kind: 'search_chart', ids: response.value.ids, quality };
  });
}
function playSearchCard(item: SearchItem) {
  const view = player.view.value;
  if (searchCardIsCurrent(item, view?.origin, searchCurrentVoiceId(view)) && player.loaded.value) {
    void player.toggleSource();
  } else if (item.kind === 'voice') {
    const selection = searchVoiceSelection(item);
    if (selection) void player.select(selection);
  } else {
    const quality = view?.qualityPreferences.quality ?? 'standard';
    void player.prepareSelection((epoch) => searchCardSelection(core, epoch, item, quality));
  }
}
const searchCollectionQueueable = computed(
  () =>
    signedIn.value &&
    player.ready.value &&
    !player.busy.value &&
    !player.queueBusy.value &&
    !player.view.value?.fm,
);
function queueSearchCard({ item, anchor }: SearchQueueRequest) {
  if (
    page.value.kind !== 'search' ||
    !searchCollectionQueueable.value ||
    (item.kind === 'playlist' && item.playlist.trackCount <= 0)
  )
    return;
  if (item.kind === 'album') albumAction({ album: item.album, action: 'next', anchor });
  else playlistActions.value?.show(item.playlist, 'next');
}
function searchCardComments(item: SearchItem) {
  if (item.kind === 'playlist')
    openComments({ kind: 'playlist', id: item.playlist.id }, item.playlist.name);
  else if (item.kind === 'album')
    openComments({ kind: 'album', id: item.album.id }, item.album.name);
  else if (item.kind === 'voice')
    openComments({ kind: 'voice', id: item.voice.id }, item.voice.name);
  else if (item.kind === 'mv' && item.mv.mvId)
    openComments({ kind: 'mv', id: item.mv.mvId }, item.mv.title);
}
function playCatalog(pages: CatalogSelectionPage[], index: number, gesture?: TrackPlayGesture) {
  const selection: SongRowDoubleClickSelection = {
    kind: 'catalog',
    pages,
    index,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  };
  void player.select(
    gesture === 'double_click' ? { kind: 'song_row_double_click', selection } : selection,
  );
}
function playAlbumCard(album: AlbumCard) {
  playResourceCard({ kind: 'album', id: album.id });
}
function playResourceCard(resource: CatalogResource) {
  const view = player.view.value;
  if (view?.origin?.kind === resource.kind && view.origin.id === resource.id && player.loaded.value)
    void player.toggleSource();
  else
    void player.prepareSelection((epoch) =>
      catalogResourceSelection(
        core,
        epoch,
        resource,
        view?.qualityPreferences.quality ?? 'standard',
      ),
    );
}
function albumAction(request: AlbumCardRequest) {
  trackActions.value?.close();
  artistActions.value?.close();
  void albumActions.value?.show(request);
}
function artistAction(request: ArtistCardRequest) {
  trackActions.value?.close();
  albumActions.value?.close();
  void artistActions.value?.show(request);
}
function catalogMembershipChanged(epoch: string, resource: CatalogResource) {
  if (account.value.status !== 'authenticated' || account.value.epoch !== epoch) return;
  collectionsPageRef.value?.checkpoint();
  collectionMemory.invalidate(
    resource.kind === 'album' ? 'saved_albums' : 'followed_artists',
    account.value.epoch,
  );
  collectionRevision.value++;
}
function playRankingPlaylist(id: string) {
  void player.togglePlaylist(id);
}
function playSearchTrack(track: TrackView, toggleCurrent: boolean) {
  if (track.missing) return;
  void player.select({
    kind: 'search_result_track',
    id: track.id,
    toggle_current: toggleCurrent,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
function playSearchResults(tracks: TrackView[], index: number, context: SearchResultPlayback) {
  if (!tracks[index] || tracks[index].missing) return;
  void player.select({
    kind: 'search_results',
    ids: tracks.map((t) => t.id),
    index,
    context,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
function playOnlineSongs(tracks: TrackView[], index: number) {
  if (!tracks[index] || tracks[index].missing) return;
  void player.select({
    kind: 'songs',
    ids: tracks.map((t) => t.id),
    index,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
function playAlbum(id: string, revision: string, startId: string | null) {
  void player.select({
    kind: 'album',
    id,
    revision,
    start_id: startId,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
}
const together = useTogether(
  undefined,
  () => {
    void music.bootstrap(false);
  },
  () => {
    music.navigate({ kind: 'together' });
  },
);
watch(
  () => {
    const value = together.view.value;
    return JSON.stringify([
      together.epoch.value,
      value?.room?.id,
      value?.attached,
      value?.ending,
      value?.pending.map((op) => op.id + ':' + op.phase).join(','),
      value?.sharedPhase,
      value?.syncError,
      value?.connection.state,
    ]);
  },
  () => {
    if (player.queueOpen.value) void player.loadQueue();
  },
);
const podcasts = usePodcasts(undefined, () => {
  void music.bootstrap(false);
});
function playPodcast(scope: PodcastScope, revision: string, startId: string | null) {
  void player.select({ kind: 'podcast', scope, revision, start_id: startId });
}
function likePlaying(track: TrackView) {
  if (track.voiceId)
    void podcasts.change('like', track.voiceId, !podcasts.liked.value.has(track.voiceId));
  else void music.like(track);
}
function trackComments(track: TrackView) {
  openComments(
    track.voiceId ? { kind: 'voice', id: track.voiceId } : { kind: 'track', id: track.id },
    track.name,
  );
}
const local = useLocal();
const localPageRef = ref<InstanceType<typeof LocalPage>>();
const localMatching = useLocalMatching(undefined, () => {
  void music.bootstrap(false);
});
watch(local.view, (value) => localMatching.setPage(value));
function playLocal(query: LocalQuery, revision: string, startId: string | null) {
  void player.select({ kind: 'local', query, revision, start_id: startId });
}
const comments = useComments(undefined, () => {
  void music.bootstrap(false);
});
function openComments(resource: CommentResource, title: string) {
  playlistDialog.value = undefined;
  music.navigate({ kind: 'comments', resource, title });
}
function playCommentResource(card: CommentResourceCard) {
  if (currentCommentResource(card, player.view.value) && player.loaded.value)
    void player.toggleSource();
  else {
    const selection = commentResourceSelection(card, player.view.value);
    if (selection) void player.select(selection);
  }
}
function openCommentResource(destination: CommentResourceDestination) {
  if (destination.kind === 'radio') openSceneRadio(destination.id);
  else if (destination.kind === 'mv') openMv(destination.id);
  else if (destination.kind === 'user')
    music.navigate({ kind: 'playlist_creator', id: destination.id });
  else openCatalog({ kind: destination.kind, id: destination.id });
}
const mv = useMv(undefined, () => {
  void music.bootstrap(false);
});
function loadMv(id: string, taskId?: string) {
  const view = downloads.view.value;
  const task =
    taskId && view?.epoch === account.value.epoch
      ? view.tasks.find(
          (item) => item.id === taskId && item.source === 'mv' && item.track.id === id,
        )
      : undefined;
  return mv.open(id, taskId, task);
}
function openMv(id: string, taskId?: string) {
  music.navigate({ kind: 'mv', id, taskId });
}
async function playMvRelated(mvId: string, songId: string) {
  await player.select({
    kind: 'mv_related',
    mv_id: mvId,
    song_id: songId,
    quality: player.view.value?.qualityPreferences.quality ?? 'standard',
  });
  if (player.view.value?.track?.id === songId && !player.error.value && !player.lyricsOpen.value)
    player.toggleLyrics();
}
const player = usePlayer(undefined, () => {
  void music.bootstrap(false);
});
const intelligence = useIntelligence(player, () => together.view.value?.attached ?? false);
watch(
  [
    () => player.view.value?.epoch,
    () => player.view.value?.entryId,
    () => player.view.value?.state,
  ],
  () => void sceneFm.poll(),
);
const trackActions = ref<InstanceType<typeof TrackActions>>();
watch(music.page, () => trackActions.value?.close());
const playlistDialog = ref<PlaylistDialogTarget>();
const queueCollectionReturn = shallowRef<HTMLElement>();
const queueCollectionOwner = ref('');
let queueDiscoverOwner: string | undefined;
let queueRoomOwner: string | undefined;
function collectQueue(value: QueueCollectionView, trigger: HTMLElement | undefined) {
  if (!signedIn.value || value.epoch !== account.value.epoch) return;
  queueCollectionReturn.value = trigger;
  queueCollectionOwner.value = value.epoch;
  playlistDialog.value = {
    kind: 'tracks',
    tracks: value.tracks,
    selectionSummary: {
      total: value.total,
      unsupported: value.unsupported,
      duplicates: value.duplicates,
    },
  };
}
function queueCollectionFallback() {
  if (!queueCollectionReturn.value) return null;
  return (
    document.querySelector<HTMLElement>('.queue-panel [aria-label="关闭播放队列"]') ??
    document.querySelector<HTMLElement>('[aria-label="播放队列"]')
  );
}
function collectionLeft() {
  if (!playlistDialog.value) {
    queueCollectionReturn.value = undefined;
    queueCollectionOwner.value = '';
  }
}
function discoverFromQueue() {
  if (!signedIn.value) {
    loginOpen.value = true;
    return;
  }
  queueDiscoverOwner = account.value.epoch;
  music.navigate({ kind: 'daily' });
  player.queueOpen.value = false;
}
function openTogetherFromQueue() {
  queueRoomOwner = account.value.epoch;
  music.navigate({ kind: 'together' });
  player.queueOpen.value = false;
}
async function queueLeft() {
  const owner = queueDiscoverOwner;
  const roomOwner = queueRoomOwner;
  queueDiscoverOwner = undefined;
  queueRoomOwner = undefined;
  await nextTick();
  if (
    roomOwner === account.value.epoch &&
    !player.queueOpen.value &&
    music.page.value.kind === 'together' &&
    !document.querySelector('dialog[open]')
  ) {
    // A clear may finish before the page's background observation catches up.
    // Read its current journal before focusing an action that could be disabled
    // by the next observation (stranding focus on the document body).
    const refreshed = await together.load(false, true);
    await nextTick();
    if (
      roomOwner !== account.value.epoch ||
      player.queueOpen.value ||
      music.page.value.kind !== 'together' ||
      document.querySelector('dialog[open]')
    )
      return;
    const active = document.activeElement;
    if (
      active !== document.body &&
      active?.getAttribute('aria-label') !== '播放队列' &&
      !active?.closest('.queue-panel')
    )
      return;
    const action =
      refreshed && together.view.value === refreshed
        ? (document.querySelector<HTMLElement>('.together-pending button:not(:disabled)') ??
          document.querySelector<HTMLElement>('.together-actions .primary:not(:disabled)'))
        : null;
    const target = action ?? document.querySelector<HTMLElement>('.together-heading h1');
    if (target) {
      if (!action) target.tabIndex = -1;
      target.focus({ preventScroll: true });
      target.scrollIntoView({ block: 'center' });
    }
    return;
  }
  if (
    owner !== account.value.epoch ||
    music.page.value.kind !== 'daily' ||
    document.querySelector('dialog[open]')
  )
    return;
  const active = document.activeElement;
  if (active !== document.body && active?.getAttribute('aria-label') !== '播放队列') return;
  const heading = document.querySelector<HTMLElement>('.page-view .daily-heading h1');
  if (heading) {
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
  }
}
const playlistEditor = ref<{ id: string; cover?: boolean }>();
const playlistEditorPage = ref<InstanceType<typeof PlaylistEditorPage>>();
const batchTarget = ref<PlaylistView>();
const batchPage = ref<InstanceType<typeof PlaylistBatchPage>>();
const batchClosing = ref(false);
let batchReturnFocus: HTMLElement | undefined,
  batchDirty = false,
  restoreBatchFocus = false;
let batchScroll = 0;
function openBatch(list: PlaylistView) {
  batchReturnFocus =
    document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  batchScroll =
    document.querySelector<HTMLElement>('.content-page-previous .track-scroll')?.scrollTop ?? 0;
  batchDirty = false;
  batchTarget.value = list;
}
async function closeBatch(restore = true) {
  if (batchClosing.value) return;
  batchClosing.value = true;
  const id = batchTarget.value?.id;
  const epoch = music.account.value.epoch;
  if (restore && batchDirty && music.page.value.kind === 'playlist' && music.page.value.id === id)
    await music.load();
  if (music.account.value.epoch !== epoch || batchTarget.value?.id !== id) return;
  restoreBatchFocus = restore;
  batchTarget.value = undefined;
  batchClosing.value = false;
}
function batchLeft() {
  if (!restoreBatchFocus || batchTarget.value) return;
  restoreBatchFocus = false;
  const scroller = document.querySelector<HTMLElement>('.content-page-previous .track-scroll');
  if (scroller) scroller.scrollTop = batchScroll;
  const target = batchReturnFocus?.isConnected
    ? batchReturnFocus
    : (document.querySelector<HTMLElement>('.playlist-hero .menu-trigger') ??
      document.querySelector<HTMLElement>('.sidebar-navigation [aria-current="page"]'));
  target?.focus({ preventScroll: true });
}
let editorReturnFocus: HTMLElement | undefined;
let editorDeleted = false;
function editPlaylist(id: string, cover = false) {
  const enter = () => {
    editorReturnFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    editorDeleted = false;
    playlistDialog.value = undefined;
    playlistEditor.value = { id, cover };
  };
  if (batchTarget.value && batchPage.value)
    batchPage.value.requestLeave(() => {
      void closeBatch(false);
      enter();
    });
  else if (cloudBatchTarget.value && cloudBatchPage.value)
    cloudBatchPage.value.requestLeave(() => {
      void closeCloudBatch(false);
      enter();
    });
  else if (playlistEditor.value && playlistEditorPage.value)
    playlistEditorPage.value.requestLeave(enter);
  else enter();
}
let restoreEditorFocus = false;
function closePlaylistEditor(restore = true) {
  restoreEditorFocus = restore;
  playlistEditor.value = undefined;
  if (editorDeleted) {
    editorDeleted = false;
    music.navigate({ kind: 'library' });
  }
}
function editorLeft() {
  if (!restoreEditorFocus || playlistEditor.value) return;
  restoreEditorFocus = false;
  const target = editorReturnFocus?.isConnected
    ? editorReturnFocus
    : document.querySelector<HTMLElement>('.playlist-edit, .playlist-card-menu button');
  target?.focus({ preventScroll: true });
}
const downloadBatch = ref<{ category: BatchCategory; query: DownloadQuery }>();
const downloadBatchPage = ref<InstanceType<typeof DownloadBatchPage>>();
let downloadBatchScroll = 0,
  restoreDownloadFocus = false;
function openDownloadBatch(category: BatchCategory) {
  downloadPageRef.value?.checkpoint();
  downloadBatchScroll = document.querySelector<HTMLElement>('.download-scroll')?.scrollTop ?? 0;
  restoreDownloadFocus = false;
  downloadBatch.value = { category, query: { ...downloads.query.value } };
}
function closeDownloadBatch(restore = true) {
  restoreDownloadFocus = restore;
  downloadBatch.value = undefined;
}
function downloadBatchLeft() {
  if (!restoreDownloadFocus || downloadBatch.value || music.page.value.kind !== 'downloads') return;
  restoreDownloadFocus = false;
  const table = document.querySelector<HTMLElement>('.download-scroll');
  if (table) table.scrollTop = downloadBatchScroll;
  document
    .querySelector<HTMLElement>('.download-toolbar .menu-trigger')
    ?.focus({ preventScroll: true });
}
const catalogDetailPage = ref<InstanceType<typeof CatalogPage>>();
const dailyQueueMessage = ref('');
const dailyQueueFailed = ref(false);
const canQueueDaily = computed(
  () =>
    player.ready.value &&
    !player.busy.value &&
    !player.queueBusy.value &&
    !music.loading.value &&
    !player.view.value?.fm &&
    !player.view.value?.intelligence &&
    !together.view.value?.attached,
);
const dailyMenuItems = computed(() => [
  { key: 'batch', label: '批量操作' },
  { key: 'queue', label: '添加全部至播放列表', disabled: !canQueueDaily.value },
]);
let dailyMenuRevision: string | null | undefined;
let restoreDailyMenu = false;
const recommendationScrollScope = computed(() => {
  const kind = music.page.value.kind;
  const revision = music.result.value?.revision;
  return revision && (kind === 'daily' || kind === 'style_recommendations')
    ? `${music.account.value.epoch}:${kind}:${revision}`
    : undefined;
});
const scrolledRecommendation = ref<string>();
const recommendationStickyVisible = computed(
  () =>
    !!recommendationScrollScope.value &&
    scrolledRecommendation.value === recommendationScrollScope.value,
);
watch(recommendationScrollScope, () => (scrolledRecommendation.value = undefined));
function recommendationScrollEdge(scope: string, scrolled: boolean, overflow: number) {
  if (scope !== recommendationScrollScope.value) return;
  const active = document.activeElement;
  const current = music.page.value.kind;
  if (scrolled) {
    const heading = document.querySelector<HTMLElement>('.page-view .page-heading');
    const actions = document.querySelector<HTMLElement>('.page-view .track-actions');
    const height = (element: HTMLElement | null) => {
      if (!element) return 0;
      const style = getComputedStyle(element);
      return (
        element.offsetHeight +
        (parseFloat(style.marginTop) || 0) +
        (parseFloat(style.marginBottom) || 0)
      );
    };
    // Short lists must still overflow once the compact bar makes the table taller.
    if (overflow <= height(heading) + height(actions) - 44) return;
    scrolledRecommendation.value = scope;
    if (active instanceof HTMLElement && active.closest('.track-actions,.page-heading-actions')) {
      const target = active.matches('.style-queue-all')
        ? '添加全部推荐歌曲至播放列表'
        : '播放全部推荐歌曲';
      void nextTick(() => {
        if (
          recommendationScrollScope.value === scope &&
          (document.activeElement === document.body || document.activeElement === active)
        )
          document
            .querySelector<HTMLElement>(
              `.recommendation-sticky [aria-label="${target}"]:not(:disabled)`,
            )
            ?.focus({ preventScroll: true });
      });
    }
    return;
  }
  const restore =
    active instanceof HTMLElement && active.closest('.recommendation-sticky')
      ? active.getAttribute('aria-label') === '播放全部推荐歌曲'
        ? '.track-actions .primary'
        : current === 'daily'
          ? '.daily-more .menu-trigger'
          : '.style-queue-all'
      : undefined;
  scrolledRecommendation.value = undefined;
  if (restore)
    void nextTick(() => {
      if (recommendationScrollScope.value === scope && document.activeElement === document.body)
        document
          .querySelector<HTMLElement>(`.page-view ${restore}:not(:disabled)`)
          ?.focus({ preventScroll: true });
    });
}
watch([music.page, () => music.account.value.epoch], ([next], [previous]) => {
  dailyQueueMessage.value = '';
  restoreDailyMenu = next.kind === 'daily' && previous.kind === 'daily_batch';
});
function restoreDailyMenuFocus() {
  if (!restoreDailyMenu || music.page.value.kind !== 'daily') return;
  const trigger = document.querySelector<HTMLElement>(
    '.page-view:not(.page-leave-active) .daily-track-actions .menu-trigger:not(:disabled)',
  );
  // Results can arrive while the old batch view is still leaving.
  if (!trigger) return;
  restoreDailyMenu = false;
  if (document.activeElement === document.body) trigger.focus({ preventScroll: true });
}
watch([music.page, music.result], () => void nextTick(restoreDailyMenuFocus), { flush: 'post' });
const styleHomeBrowser = ref<InstanceType<typeof StyleHomeBrowser>>();
const styleHomeHeading = ref<HTMLHeadingElement>();
let styleHomeFocus: 'entry' | 'heading' | undefined;
watch(
  () => music.page.value,
  (next, prior) => {
    styleHomeFocus =
      next.kind === 'style_recommendations' && prior.kind === 'style_recommendations'
        ? next.seed
          ? 'heading'
          : prior.seed
            ? 'entry'
            : undefined
        : undefined;
  },
);
function mainPageEntered() {
  miniFmFocus.complete();
  restoreDailyMenuFocus();
  const focus = styleHomeFocus;
  styleHomeFocus = undefined;
  if (document.activeElement !== document.body) return;
  if (focus === 'entry') styleHomeBrowser.value?.focus();
  if (focus === 'heading') styleHomeHeading.value?.focus({ preventScroll: true });
}
async function dailyMenuAction(key: string) {
  const currentPage = music.page.value;
  const epoch = music.account.value.epoch;
  const revision = music.result.value?.revision;
  if (
    currentPage.kind !== 'daily' ||
    !revision ||
    dailyMenuRevision !== revision ||
    music.loading.value
  )
    return;
  if (key === 'batch') return music.navigate({ kind: 'daily_batch' });
  if (key === 'queue') await queueRecommendations();
}
async function queueRecommendations() {
  const currentPage = music.page.value;
  const epoch = music.account.value.epoch;
  const revision = music.result.value?.revision;
  const style = currentPage.kind === 'style_recommendations';
  if ((!style && currentPage.kind !== 'daily') || !revision || !canQueueDaily.value) return;
  const visible = () => music.page.value === currentPage && music.account.value.epoch === epoch;
  const starts = !player.view.value?.queueLen && !player.view.value?.track;
  dailyQueueFailed.value = false;
  dailyQueueMessage.value = '正在添加至播放列表…';
  try {
    const state = await (style
      ? player.queueStyleRecommendations(revision)
      : player.queueDaily(revision));
    if (!visible()) return;
    if (!state) throw { message: '播放状态已变化，请重试' };
    if (starts && state.state === 'error') throw { message: '歌曲未能播放，请重试' };
    dailyQueueMessage.value = starts
      ? `已开始播放${style ? '风格推荐' : '每日推荐'}`
      : '已添加全部至播放列表';
  } catch (error) {
    if (!visible()) return;
    const issue = failure(error);
    dailyQueueFailed.value = true;
    dailyQueueMessage.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) void music.bootstrap(false);
  }
}
async function retryDailyCache(event: MouseEvent) {
  const trigger = event.currentTarget;
  if (!(trigger instanceof HTMLButtonElement)) return;
  const ownsFocus = document.activeElement === trigger;
  const epoch = music.account.value.epoch;
  const currentPage = music.page.value;
  const heading = trigger.closest('.main-content')?.querySelector<HTMLElement>('.daily-heading h1');
  await music.retryDailyCache();
  await nextTick();
  if (
    ownsFocus &&
    !trigger.isConnected &&
    heading?.isConnected &&
    document.activeElement === document.body &&
    music.account.value.epoch === epoch &&
    music.page.value === currentPage &&
    music.signedIn.value
  ) {
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
  }
}
function openDailyHistory() {
  if (!signedIn.value) {
    loginOpen.value = true;
    return;
  }
  void member.open({ kind: 'history', date: null });
  music.navigate({ kind: 'member' });
}
function openStyleRecommendations() {
  if (!signedIn.value) {
    loginOpen.value = true;
    return;
  }
  music.navigate({ kind: 'style_recommendations' });
}
function openHomeStyle(seed: StyleSeedHandle) {
  if (
    music.signedIn.value &&
    music.page.value.kind === 'style_recommendations' &&
    !music.page.value.seed
  )
    music.navigate({ kind: 'style_recommendations', seed });
}
function backFromPage() {
  if (batchTarget.value) batchPage.value?.requestLeave();
  else if (downloadBatch.value) downloadBatchPage.value?.requestLeave();
  else if (cloudBatchTarget.value) cloudBatchPage.value?.requestLeave();
  else if (playlistEditor.value) playlistEditorPage.value?.requestLeave();
  else music.back();
}
const downloadDialog = ref<DownloadDialogTarget>();
function openCurrentDownload(epoch: string, playId: string, trackId: string) {
  const current = currentCatalogDownload(player.view.value);
  if (
    !signedIn.value ||
    downloadDialog.value ||
    epoch !== account.value.epoch ||
    current?.epoch !== epoch ||
    current.playId !== playId ||
    current.track.id !== trackId
  )
    return;
  downloadDialog.value = { kind: 'start', tracks: [current.track] };
}

function downloadReturnFocus() {
  return (
    document.querySelector<HTMLElement>('.download-mv-cover[tabindex="0"]') ??
    document.querySelector<HTMLElement>('.download-tabs [aria-selected="true"]')
  );
}
const downloads = useDownloads(undefined, () => {
  void music.bootstrap(false);
});
const downloadPageRef = ref<InstanceType<typeof DownloadPage>>();
watch(
  music.page,
  (next, previous) => {
    albumActions.value?.close();
    artistActions.value?.close();
    if (previous.kind === 'rankings') {
      checkpointRanking(previous);
      rankings.leave();
    }
    if (previous.kind === 'local' && next.kind !== 'local') localPageRef.value?.checkpoint();
    if (previous.kind === 'downloads') {
      downloadPageRef.value?.checkpoint();
      downloads.leaveVisit();
    }
    if (next.kind === 'downloads')
      downloads.enterVisit(
        next,
        next.initialTab === 'active' ? downloadSectionQuery('active') : undefined,
      );
  },
  { flush: 'sync' },
);
watch(
  music.initializing,
  (value) => {
    const target = music.page.value;
    if (target.kind !== 'rankings') return;
    if (value) {
      checkpointRanking(target);
      rankings.leave();
    } else void rankings.enter(target.query, target.rankingState);
  },
  { flush: 'sync' },
);
async function queueDownload(task: import('./services/downloads').DownloadTask) {
  const view = player.view.value,
    owner = account.value.epoch;
  if (!view || view.fm || player.busy.value) return;
  const result = await player.action({
    kind: 'queue_downloads',
    task_ids: [task.id],
    expected_queue_revision: view.queueRevision,
  });
  if (owner === account.value.epoch) {
    if (result) downloads.notice.value = '播放列表已更新';
    else if (player.error.value) downloads.error.value = '下一首播放：' + player.error.value;
  }
}
const cloud = useCloud(undefined, () => {
  void music.bootstrap(false);
});
const cloudDialogOpen = ref(false);
const cloudBatchTarget = ref<{ epoch: string }>();
const cloudBatchPage = ref<InstanceType<typeof CloudBatchPage>>();
const cloudBatchClosing = ref(false);
let cloudBatchOrigin: HTMLElement | undefined,
  cloudBatchScroll = 0,
  cloudBatchQuery = '',
  cloudBatchOwner = '',
  restoreCloudBatch = false;
function openCloudBatch() {
  if (cloud.busy.value || cloud.loading.value || !cloud.page.value?.revision) return;
  cloudBatchOrigin =
    document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  cloudBatchScroll = document.querySelector<HTMLElement>('.cloud-page')?.scrollTop ?? 0;
  cloudBatchQuery = JSON.stringify(cloud.query.value);
  cloudBatchOwner = music.account.value.epoch;
  cloudBatchTarget.value = { epoch: cloudBatchOwner };
}
async function closeCloudBatch(restore = true) {
  if (cloudBatchClosing.value) return;
  const owner = cloudBatchTarget.value?.epoch;
  if (!owner) return;
  cloudBatchClosing.value = true;
  if (restore) await cloud.refresh(false, true);
  if (music.account.value.epoch !== owner || cloudBatchTarget.value?.epoch !== owner) return;
  restoreCloudBatch = restore;
  cloudBatchTarget.value = undefined;
  cloudBatchClosing.value = false;
}
function cloudBatchLeft() {
  if (!restoreCloudBatch || cloudBatchTarget.value) return;
  restoreCloudBatch = false;
  if (music.account.value.epoch !== cloudBatchOwner || music.page.value.kind !== 'cloud') return;
  const root = document.querySelector<HTMLElement>('.cloud-page');
  if (root)
    root.scrollTop = JSON.stringify(cloud.query.value) === cloudBatchQuery ? cloudBatchScroll : 0;
  const active = document.activeElement;
  if (
    active !== document.body &&
    active !== cloudBatchOrigin &&
    !(active as HTMLElement)?.closest('.cloud-batch-page')
  )
    return;
  const target =
    cloudBatchOrigin?.isConnected && !cloudBatchOrigin.matches(':disabled')
      ? cloudBatchOrigin
      : root?.querySelector<HTMLElement>('[data-cloud-toolbar-more] .menu-trigger');
  target?.focus({ preventScroll: true });
}

const playlistManager = usePlaylists(
  undefined,
  async (change) => {
    music.notice.value = change.message;
    await music.resources();
    if (
      playlistSaved(change.phase) &&
      change.kind === 'create' &&
      change.playlistId &&
      playlistDialog.value?.kind !== 'tracks' &&
      !batchTarget.value &&
      !downloadBatch.value
    ) {
      music.navigate({ kind: 'playlist', id: change.playlistId });
    } else if (music.page.value.kind === 'playlist' && !batchTarget.value) {
      if (
        playlistSaved(change.phase) &&
        change.kind === 'delete' &&
        music.page.value.id === change.playlistId
      )
        if (playlistEditor.value) editorDeleted = true;
        else music.navigate({ kind: 'library' });
      else await music.load();
    }
  },
  () => {
    void music.bootstrap(false);
  },
);
const {
  account,
  playlists,
  liked,
  busyLikes,
  page,
  result,
  loading,
  initializing,
  error,
  notice,
  libraryError,
  historyLength,
  signedIn,
} = music;
const playlistCreator = usePlaylistCreator(core);
watch(
  [
    () => account.value.epoch,
    () => account.value.status,
    () => (page.value.kind === 'playlist_creator' ? page.value.id : null),
  ],
  ([epoch, status, uid]) => {
    void playlistCreator.enter(status === 'authenticated' ? epoch : '', uid);
  },
  { immediate: true, flush: 'sync' },
);
const playlistTagBrowse = usePlaylistTagBrowse(core);
watch(
  [
    () => account.value.epoch,
    () => account.value.status,
    () => (page.value.kind === 'playlist_tag' ? page.value.tag : null),
  ],
  ([epoch, status, tag]) => {
    void playlistTagBrowse.enter(status === 'authenticated' ? epoch : '', tag);
  },
  { immediate: true, flush: 'sync' },
);
const playlistPanelId = () =>
  page.value.kind === 'playlist' && result.value?.playlist?.id === page.value.id
    ? page.value.id
    : null;
const subscribers = usePlaylistSubscribers(core, () => account.value.epoch, playlistPanelId);
let subscriberReturnSerial = 0;
type SubscriberFocus = { row: number; scrollTop: number };
let subscriberReturn:
  | {
      token: number;
      epoch: string;
      playlistId: string;
      page: NonNullable<typeof subscribers.value.value>;
      focus: SubscriberFocus;
    }
  | undefined;
const subscriberFocus = shallowRef<SubscriberFocus>();
watch(
  [() => account.value.epoch, () => account.value.status],
  () => {
    subscriberReturn = undefined;
    subscriberFocus.value = undefined;
  },
  { flush: 'sync' },
);
watch(
  [playlistPanelId, () => page.value],
  ([id, route]) => {
    subscriberFocus.value = undefined;
    if (
      id &&
      route.kind === 'playlist' &&
      subscriberReturn?.token === route.subscriberReturn &&
      subscriberReturn?.epoch === account.value.epoch &&
      subscriberReturn.playlistId === id
    ) {
      const saved = subscriberReturn;
      subscriberReturn = undefined;
      subscriberFocus.value = saved.focus;
      subscribers.restore(saved.page);
    }
  },
  { flush: 'sync' },
);
function openSubscriber(id: string, focus: SubscriberFocus) {
  const playlistId = playlistPanelId();
  const current = subscribers.value.value;
  if (
    !signedIn.value ||
    !playlistId ||
    !subscribers.open.value ||
    subscribers.busy.value ||
    !current ||
    current.subscribers[focus.row]?.userId !== id
  )
    return;
  const token = ++subscriberReturnSerial;
  subscriberReturn = { token, epoch: account.value.epoch, playlistId, page: current, focus };
  music.openSubscriberProfile(id, token);
}
const playlistComments = usePlaylistCommentTab(
  core,
  () => account.value,
  playlistPanelId,
  () => {
    void music.bootstrap(false);
  },
);
const playlistSongsActive = computed(
  () => !subscribers.open.value && !playlistComments.active.value,
);
function showPlaylistSongs() {
  subscribers.hide();
  playlistComments.hide();
}
function showPlaylistComments() {
  subscribers.hide();
  void playlistComments.show();
}
function showPlaylistSubscribers() {
  playlistComments.hide();
  void subscribers.show();
}
const playlistKeyword = ref('');
const playlistTrackSearch = computed(() =>
  filterTrackScope(
    result.value?.tracks ?? [],
    page.value.kind === 'playlist' && result.value?.playlist?.id === page.value.id
      ? playlistKeyword.value
      : '',
  ),
);
watch(
  [() => account.value.epoch, () => (page.value.kind === 'playlist' ? page.value.id : null)],
  () => (playlistKeyword.value = ''),
  { flush: 'sync' },
);
const loginOpen = ref(false);
const coldOffline = useColdOffline(core);
const coldOfflineEligible = computed(
  () =>
    music.accountReady.value &&
    !initializing.value &&
    !loginOpen.value &&
    account.value.status === 'unknown' &&
    page.value.kind === 'downloads',
);
watch(coldOfflineEligible, coldOffline.setEligible, { flush: 'sync' });
watch(() => account.value.epoch, coldOffline.leave, { flush: 'sync' });
onUnmounted(coldOffline.leave);

const lyricReport = useLyricReport(
  () => account.value.epoch,
  createLyricReportClient(),
  (message) => {
    notice.value = message;
  },
);
function reportLyrics() {
  if (!signedIn.value) {
    loginOpen.value = true;
    return;
  }
  const id = player.view.value?.playId;
  if (id) void lyricReport.show(id);
}
function lyricPanelEnter(element: Element) {
  (element as HTMLElement).inert = false;
}
function lyricPanelLeave(element: Element) {
  const active = document.activeElement;
  if (active instanceof HTMLElement && element.contains(active)) active.blur();
  (element as HTMLElement).inert = true;
}
const accountsOpen = ref(false);
const accountFocus = useModalOrigin(
  () => loginOpen.value || accountsOpen.value,
  () => document.querySelector<HTMLElement>('.account-button'),
);
// A successful login can close its dialog while account bootstrap still
// disables the original page button. Recover only unowned focus after the
// current bootstrap finishes; never steal it from another modal or control.
watch(
  initializing,
  (pending) => {
    if (!pending) accountFocus.restoreIfUnowned();
  },
  { flush: 'post' },
);
const favorite = computed(() => playlists.value.find((p) => p.favorite && p.owned));
const playlistActions = ref<InstanceType<typeof PlaylistActions>>();
function playlistMenu(list: PlaylistView, scene: PlaylistMenuScene = 'collection') {
  return playlistMenuItems(list, scene, {
    lists: playlists.value,
    busy: playlistManager.busy.value || downloads.busy.value || !!playlistActions.value?.busy,
    playable: player.ready.value,
    preparing: player.busy.value || player.queueBusy.value,
    fm: player.view.value?.fm,
  });
}
function playlistAction(list: PlaylistView, key: string, scene: PlaylistMenuScene = 'collection') {
  const item = playlistMenu(list, scene).find((item) => item.key === key);
  if (!item || item.disabled) return;
  if (key === 'view') music.navigate({ kind: 'playlist', id: list.id });
  else if (key === 'batch') openBatch(list);
  else if (key === 'play') playMemberPlaylist(list.id, null);
  else if (key === 'edit') editPlaylist(list.id);
  else if (key === 'up' || key === 'down')
    void playlistManager.move(list.id, list.owned, key === 'up' ? -1 : 1, favorite.value?.id);
  else playlistActions.value?.show(list, key);
}
watch(
  () => player.view.value?.track?.voiceId,
  (voice) => {
    if (voice && !podcasts.likesKnown.value && !podcasts.loading.value) void podcasts.state(true);
  },
);
// Stable scalar dependencies keep playback clock ticks out of the page tree.
const currentTrackId = computed(() => player.view.value?.track?.id);
const currentSearchSongId = computed(() => searchCurrentSongId(player.view.value));
const playlistTrackTable = ref<InstanceType<typeof TrackTable>>();
const locatableSongId = computed(() => {
  const current = player.view.value;
  if (
    current?.playId &&
    current.track &&
    !current.track.voiceId &&
    !current.track.local &&
    (current.resourceKind === 'catalog' || current.resourceKind === 'downloaded')
  )
    return current.track.id;
});
const playlistLocateIndex = computed(() => {
  const id = locatableSongId.value;
  if (
    !id ||
    page.value.kind !== 'playlist' ||
    result.value?.playlist?.id !== page.value.id ||
    !playlistSongsActive.value
  )
    return undefined;
  const index = playlistTrackSearch.value.tracks.findIndex(
    (track) => track.id === id && !track.missing && !track.local,
  );
  return index >= 0 ? index : undefined;
});
const currentPlaylistId = computed(() =>
  player.view.value?.origin?.kind === 'playlist' ? player.view.value.origin.id : undefined,
);
const libraryPlaying = player.playing;
function refreshLibrary() {
  notice.value = '';
  void music.resources();
}
function playLibraryPlaylist(list: PlaylistView) {
  if (currentPlaylistId.value === list.id && player.loaded.value) void player.toggleSource();
  else playMemberPlaylist(list.id, null);
}
const currentLocalId = computed(() =>
  player.view.value?.track?.local ? currentTrackId.value : undefined,
);
const currentVoiceId = computed(() => player.view.value?.track?.voiceId);
// Intern scalar identity so clock updates cannot rerender all search cards.
const currentOriginKey = computed(() => {
  const origin = player.view.value?.origin;
  return origin ? `${origin.kind}:${origin.id}` : '';
});
const currentOrigin = computed(() => {
  const [kind, id] = currentOriginKey.value.split(':');
  return id ? { kind: kind as import('./services/player').PlayerOrigin['kind'], id } : null;
});
const playbackState = computed(() => player.view.value?.state);
const navigationKey = computed(() => {
  const current = page.value;
  if (current.kind !== 'search') return JSON.stringify(current);
  const { pagination, ...route } = current;
  return JSON.stringify({ ...route, offset: pagination?.offset, number: pagination?.number });
});
const pageMotion = useMotion();
const pageTransition = createPageTransition(() => pageMotion.reduced.value);
onUnmounted(pageTransition.dispose);
const pageKey = computed(() =>
  JSON.stringify({
    account: page.value.kind === 'local' ? undefined : [account.value.epoch, account.value.status],
    page:
      page.value.kind === 'search'
        ? { kind: 'search', keyword: page.value.keyword }
        : page.value.kind === 'catalog'
          ? {
              kind: 'catalog',
              resource: page.value.resource,
              catalogVisit: page.value.catalogVisit,
            }
          : page.value.kind === 'collections'
            ? { kind: 'collections', collectionVisit: page.value.collectionVisit }
            : page.value.kind === 'settings'
              ? { kind: 'settings' }
              : page.value,
  }),
);
const playingLikes = computed(() => {
  const voiceId = currentVoiceId.value;
  const trackId = currentTrackId.value;
  return voiceId && trackId
    ? new Set(podcasts.liked.value.has(voiceId) ? [trackId] : [])
    : liked.value;
});
const playingLikeBusy = computed(() => {
  const voiceId = currentVoiceId.value;
  const trackId = currentTrackId.value;
  return voiceId && trackId
    ? new Set(
        podcasts.busy.value || !podcasts.likesKnown.value || podcasts.pending('like', voiceId)
          ? [trackId]
          : [],
      )
    : busyLikes.value;
});
const title = computed(() =>
  page.value.kind === 'daily'
    ? '每日推荐'
    : page.value.kind === 'style_recommendations'
      ? page.value.seed
        ? '首页风格推荐'
        : '风格推荐'
      : page.value.kind === 'library'
        ? '我的歌单'
        : page.value.kind === 'search'
          ? `搜索「${page.value.keyword}」`
          : (result.value?.playlist?.name ?? '歌单'),
);
async function authorized() {
  loginOpen.value = false;
  accountsOpen.value = false;
  await music.bootstrap(false);
}
let accountListener: (() => void) | undefined;
const likeRecovery = useLikeRecovery(
  core,
  () => {
    void music.bootstrap(false);
  },
  () => {
    void music.refreshLikes();
  },
);
const likeRecoveryFocus = () => document.querySelector<HTMLButtonElement>('.settings-button');
watch(music.likeActivity, () => {
  void likeRecovery.refresh();
});
let nativeLikeListener: (() => void) | undefined;
let desktopLyricListener: (() => void) | undefined;
let lyricReportListener: (() => void) | undefined;
let miniFmListener: (() => void) | undefined;
const miniFmFocus = useMiniFmFocus(
  fm,
  () => account.value,
  () => music.page.value.kind,
);
let mounted = true;
onMounted(async () => {
  const localIdentity = music.primeLocalAccount();
  try {
    const stop = await core.onAccountChanged((next) => {
      if (
        next.epoch !== account.value.epoch ||
        next.status !== account.value.status ||
        next.canSignOut !== account.value.canSignOut
      ) {
        void music.bootstrap(false);
      } else {
        account.value.nickname = next.nickname;
        account.value.avatarUrl = next.avatarUrl;
      }
    });
    if (mounted) accountListener = stop;
    else stop();
  } catch {
    notice.value = '账号状态订阅失败，请重新启动客户端';
  }
  try {
    const stop = await core.onNativeLikeChanged((value) => {
      if (
        !mounted ||
        value.epoch !== account.value.epoch ||
        account.value.status !== 'authenticated'
      )
        return;
      if (value.resource.kind === 'voice') void podcasts.externalLikeChanged();
      else void music.refreshLikes();
      void likeRecovery.refresh();
    });
    if (mounted) nativeLikeListener = stop;
    else stop();
  } catch {
    notice.value = '托盘喜欢状态订阅失败，请刷新资源或重新启动客户端';
  }
  try {
    const stop = await onLyricReportOpen((value) => {
      if (mounted && value.epoch === account.value.epoch) void lyricReport.showId(value.id);
    });
    if (mounted) lyricReportListener = stop;
    else stop();
  } catch {
    /* The lyric panel remains an independent entry. */
  }
  try {
    const stop = await onDesktopLyricOpen(async (value) => {
      if (!mounted || value.epoch !== account.value.epoch) return;
      if (value.settings) {
        player.lyricsOpen.value = false;
        music.navigate({ kind: 'settings' });
        await nextTick();
        document.querySelector('#settings-desktop-lyrics')?.scrollIntoView({ block: 'start' });
      } else if (value.playId && value.playId === player.view.value?.playId) {
        player.lyricsOpen.value = true;
        void player.loadLyrics();
      }
    });
    if (mounted) desktopLyricListener = stop;
    else stop();
  } catch {
    /* The settings entry remains available if event subscription fails. */
  }
  try {
    const stop = await onMiniReturn(async (value) => {
      if (
        !mounted ||
        value.epoch !== account.value.epoch ||
        account.value.status !== 'authenticated'
      )
        return;
      if (!value.fm) {
        if (music.page.value.kind === 'fm') await fm.load(true);
        return;
      }
      const wasFm = music.page.value.kind === 'fm';
      miniFmFocus.request(value.epoch);
      music.navigate({ kind: 'fm' });
      if (wasFm) await fm.load(true);
      await nextTick();
      miniFmFocus.complete();
    });
    if (mounted) miniFmListener = stop;
    else stop();
  } catch {
    /* The sidebar remains available when an event listener fails. */
  }
  await localIdentity;
  if (mounted) await music.bootstrap();
});
onUnmounted(() => {
  catalogWorkspace.clear();
  collectionMemory.clear();
  mounted = false;
  accountListener?.();
  nativeLikeListener?.();
  desktopLyricListener?.();
  lyricReportListener?.();
  miniFmListener?.();
});
watch([() => account.value.epoch, () => account.value.status], () => {
  playlistDialog.value = undefined;
  playlistEditor.value = undefined;
  if (
    cloudBatchTarget.value &&
    (cloudBatchTarget.value.epoch !== account.value.epoch ||
      account.value.status !== 'authenticated')
  ) {
    cloudBatchTarget.value = undefined;
    cloudBatchClosing.value = false;
    restoreCloudBatch = false;
  }
  batchTarget.value = undefined;
  downloadBatch.value = undefined;
  restoreDownloadFocus = false;
  batchClosing.value = false;
  restoreBatchFocus = false;
  batchReturnFocus = undefined;
  playlistLibraryState.value = libraryView();
  editorDeleted = false;
  playlistManager.connect(account.value);
  likeRecovery.connect(account.value);
  downloadDialog.value = undefined;
  void downloads.connect(account.value);
  void cloud.connect(account.value);
  mv.connect(account.value);
  podcasts.connect(account.value);
  together.connect(account.value);
  rankings.connect(account.value);
  fm.connect(account.value);
  sceneFm.connect(account.value);
  member.connect(account.value);
  if (page.value.kind === 'rankings' && !music.initializing.value)
    void rankings.enter(page.value.query, page.value.rankingState);
  comments.connect(account.value);
  localMatching.connect(account.value);
  if (page.value.kind === 'mv') void loadMv(page.value.id, page.value.taskId);
  if (page.value.kind === 'comments') void comments.open(page.value.resource);
});
watch([() => account.value.epoch, music.accountReady], () => {
  if (music.accountReady.value) void player.connect(account.value);
});
onUnmounted(() => local.dispose());
onUnmounted(() => likeRecovery.dispose());
onUnmounted(() => localMatching.dispose());
onUnmounted(() => player.dispose());
onUnmounted(() => downloads.dispose());
onUnmounted(() => cloud.dispose());
onUnmounted(() => mv.dispose());
onUnmounted(() => podcasts.dispose());
onUnmounted(() => together.dispose());
onUnmounted(() => rankings.dispose());
onUnmounted(() => fm.dispose());
onUnmounted(() => sceneFm.dispose());
onUnmounted(() => member.dispose());
onUnmounted(() => recent.dispose());
onUnmounted(() => settings.dispose());
onUnmounted(() => relay.dispose());
onUnmounted(() => publication.dispose());
watch(
  () => page.value.kind === 'settings',
  (value) => publication.setVisible(value),
  { immediate: true },
);
watch(
  () => [account.value.epoch, account.value.status, initializing.value],
  () => {
    if (
      !initializing.value &&
      relay.connect(account.value) &&
      account.value.status === 'authenticated'
    )
      void relay.read(true);
  },
  { immediate: true },
);
watch(
  () => [account.value.epoch, account.value.status],
  () => {
    recent.connect(account.value);
    settings.connect(account.value);
    publication.connect(account.value);
    if (page.value.kind === 'settings' && !page.value.accountDetail) void settings.open();
    if (page.value.kind === 'history') void recent.open(page.value.scope);
  },
  { immediate: true },
);
watch(
  () => page.value,
  (value) => {
    mv.show(value.kind === 'mvs');
    if (value.kind === 'mv') void loadMv(value.id, value.taskId);
    if (value.kind === 'comments') void comments.open(value.resource);
    if (value.kind === 'rankings') void rankings.enter(value.query, value.rankingState);
    if (value.kind === 'history') void recent.open(value.scope);
    if (value.kind === 'settings' && !value.accountDetail) void settings.open();
  },
);
watch([() => player.view.value?.entryId, () => player.view.value?.state], () => {
  if (page.value.kind === 'history' && !recent.loading.value) void recent.localRefresh();
});
watch(
  () => page.value.kind,
  (kind) => {
    cloudDialogOpen.value = false;
    void cloud.show(kind === 'cloud');
    local.show(kind === 'local');
    podcasts.show(kind === 'podcasts');
    together.show(kind === 'together');
    fm.show(kind === 'fm');
    void sceneFm.show(kind === 'scene_fm');
    member.show(kind === 'member');
  },
);
watch(
  () => [
    page.value.kind,
    page.value.kind === 'downloads' ? page.value.downloadVisit : undefined,
    !!downloadDialog.value,
    !!playlistActions.value?.downloadOpen,
  ],
  () =>
    downloads.show(
      page.value.kind === 'downloads' ||
        !!downloadDialog.value ||
        !!playlistActions.value?.downloadOpen,
    ),
);
function showVisibleTrackActions(request: TrackMenuRequest) {
  const playlist = result.value?.playlist;
  if (!playlist) {
    const snapshot = result.value;
    const kind = page.value.kind;
    if ((kind !== 'daily' && kind !== 'style_recommendations') || !snapshot) {
      trackActions.value?.show(request);
      return;
    }
    trackActions.value?.show({
      ...request,
      play: () => {
        if (page.value.kind !== kind || result.value !== snapshot) return;
        const index = snapshot.tracks.indexOf(request.track);
        if (index >= 0) play(index);
      },
    });
    return;
  }
  const epoch = account.value.epoch;
  trackActions.value?.show(
    {
      ...request,
      play: () => {
        if (
          account.value.epoch !== epoch ||
          page.value.kind !== 'playlist' ||
          page.value.id !== playlist.id ||
          result.value?.playlist?.id !== playlist.id
        )
          return;
        const index = result.value.tracks.indexOf(request.track);
        if (index >= 0) play(index);
      },
    },
    playlist,
  );
}
function playVisibleTrack(index: number, gesture?: TrackPlayGesture) {
  const positions = playlistTrackSearch.value.positions;
  const sourceIndex = positions === null ? index : positions[index];
  if (sourceIndex !== undefined) play(sourceIndex, gesture);
}
function play(index = 0, gesture?: TrackPlayGesture) {
  const tracks = result.value?.tracks;
  if (!tracks?.[index] || tracks[index].missing) return;
  const quality = player.view.value?.qualityPreferences.quality ?? 'standard';
  if (page.value.kind === 'daily' || page.value.kind === 'style_recommendations') {
    const revision = result.value?.revision;
    if (revision) {
      const kind = page.value.kind === 'daily' ? 'daily_recommendations' : 'style_recommendations';
      void player.select({ kind, revision, index, quality });
    }
  } else if (page.value.kind === 'playlist') {
    const selection: SongRowDoubleClickSelection = {
      kind: 'playlist',
      id: page.value.id,
      start_id: tracks[index].id,
      revision: result.value?.revision ?? null,
      quality,
    };
    void player.select(
      gesture === 'double_click' ? { kind: 'song_row_double_click', selection } : selection,
    );
  } else void player.select({ kind: 'songs', ids: tracks.map((t) => t.id), index, quality });
}
const shortcutKeys = useLocalShortcuts((error) => {
  notice.value = error.message;
});
watch(
  () => account.value.epoch,
  (epoch) => shortcutKeys.account(epoch),
);
function keyboard(event: KeyboardEvent) {
  if (
    shortcutKeys.handle(
      event,
      !!(
        loginOpen.value ||
        accountsOpen.value ||
        playlistDialog.value ||
        downloadDialog.value ||
        cloudDialogOpen.value
      ),
      {
        audioPlayId: player.view.value?.playId ?? null,
        audioEntryId: player.view.value?.entryId ?? null,
        videoPlayId: mv.video.value?.playId ?? null,
      },
    )
  )
    return;
  // Older synthetic hosts have no native shortcut service. Keep their existing
  // space-key route; an installed service owns dispatch, including failures.
  if (shortcutKeys.native.view.value && shortcutKeys.native.view.value.phase !== 'not_installed')
    return;
  if (
    event.code !== 'Space' ||
    event.defaultPrevented ||
    event.isComposing ||
    event.repeat ||
    event.shiftKey ||
    event.ctrlKey ||
    event.altKey ||
    event.metaKey ||
    loginOpen.value ||
    accountsOpen.value ||
    playlistDialog.value ||
    downloadDialog.value ||
    cloudDialogOpen.value ||
    document.querySelector('dialog[open], [aria-modal="true"]')
  )
    return;
  const target = event.target;
  if (
    !(target instanceof HTMLElement) ||
    target.isContentEditable ||
    target.closest('input,textarea,select,button,[role="textbox"]')
  )
    return;
  if (mv.video.value?.id && (page.value.kind === 'mv' || mv.video.value.state === 'playing')) {
    event.preventDefault();
    void mv.action(mv.video.value.paused ? { kind: 'play' } : { kind: 'pause', paused: true });
    return;
  }
  if (
    player.ready.value &&
    (player.view.value?.queueLen || player.view.value?.track) &&
    !player.busy.value
  ) {
    event.preventDefault();
    void player.action(player.playing.value ? { kind: 'pause', paused: true } : { kind: 'play' });
  }
}
const playerResume = createPlayerResume(player, () => document.visibilityState === 'visible');
const resumeNativePlayer = () => {
  void playerResume.wake();
};
onMounted(() => {
  window.addEventListener('keydown', keyboard);
  window.addEventListener('focus', resumeNativePlayer);
  document.addEventListener('visibilitychange', resumeNativePlayer);
  void shortcutKeys.connect(account.value.epoch);
});
onUnmounted(() => {
  window.removeEventListener('keydown', keyboard);
  window.removeEventListener('focus', resumeNativePlayer);
  document.removeEventListener('visibilitychange', resumeNativePlayer);
  playerResume.dispose();
  shortcutKeys.dispose();
});
</script>
<template>
  <div class="app-shell">
    <AppSidebar
      :key="`${account.epoch}:${account.status}`"
      :page="page.kind === 'daily_batch' ? { kind: 'daily' } : page"
      :playlists="playlists"
      :history-kind="recent.kind.value"
      :initializing="initializing"
      :loading="music.resourcesLoading.value"
      :error="music.playlistError.value"
      :playing-id="libraryPlaying ? currentPlaylistId : undefined"
      :items="playlistMenu"
      @navigate="music.navigate"
      @ranking="openRanking({ kind: 'home' })"
      @refresh="refreshLibrary"
      @create="signedIn ? (playlistDialog = { kind: 'create' }) : (loginOpen = true)"
      @action="playlistAction"
    />
    <header class="topbar" data-tauri-drag-region>
      <button
        class="back-button icon-button"
        aria-label="返回上一页"
        :disabled="
          !historyLength && !playlistEditor && !batchTarget && !downloadBatch && !cloudBatchTarget
        "
        @click="backFromPage"
      >
        <Icon name="back" />
      </button>
      <SearchBox
        ref="searchBox"
        :account="account"
        :page-key="navigationKey"
        :keyword="page.kind === 'search' ? page.keyword : undefined"
        :display-keyword="page.kind === 'search' ? page.displayKeyword : undefined"
        :playable="player.ready.value && !player.busy.value"
        @play-chart="playSearchChart"
        @radio="openSearchChartRadio"
        @search="
          (word, channel, displayKeyword) =>
            music.navigate({ kind: 'search', keyword: word, channel, displayKeyword })
        "
        @expired="music.bootstrap(false)"
      />
      <div class="header-spacer" data-tauri-drag-region />
      <button
        class="account-button"
        :title="signedIn ? (account.nickname ?? '我的账号') : '点击登录'"
        :disabled="initializing"
        @click="signedIn ? (accountsOpen = true) : (loginOpen = true)"
      >
        <img
          v-if="account.avatarUrl"
          :src="account.avatarUrl"
          alt=""
          referrerpolicy="no-referrer"
        /><span v-else class="avatar"><Icon name="user" /></span
        ><span>{{ signedIn ? (account.nickname ?? '我的账号') : '点击登录' }}</span>
      </button>
      <AppearanceMenu />
      <button
        class="icon-button settings-button"
        :disabled="initializing"
        aria-label="打开设置"
        title="设置"
        :aria-pressed="page.kind === 'settings'"
        @click="music.navigate({ kind: 'settings' })"
      >
        <Icon name="settings" />
      </button>
      <WindowControls />
    </header>
    <main class="main-content">
      <LikeRecoveryNotice :model="likeRecovery" />
      <div class="content-page-stack">
        <Transition name="page" @after-leave="batchLeft">
          <PlaylistBatchPage
            v-if="batchTarget"
            ref="batchPage"
            :key="account.epoch + ':' + batchTarget.id"
            :playlist="batchTarget"
            :account="account"
            :manager="playlistManager"
            :player="player"
            :downloads="downloads"
            :playlists="playlists"
            :leaving="batchClosing"
            :liked="liked"
            @close="closeBatch()"
            @expired="music.bootstrap(false)"
            @changed="batchDirty = true"
            @downloaded="music.navigate({ kind: 'downloads', initialTab: 'active' })"
          />
        </Transition>
        <Transition name="page" @after-leave="editorLeft">
          <PlaylistEditorPage
            v-if="playlistEditor"
            ref="playlistEditorPage"
            :key="account.epoch + ':' + playlistEditor.id"
            :target="playlistEditor"
            :manager="playlistManager"
            :account="account"
            @close="closePlaylistEditor()"
            @expired="music.bootstrap(false)"
            @cover-changed="
              async () => {
                await music.resources();
                if (page.kind === 'playlist') await music.load();
              }
            "
          />
        </Transition>
        <Transition name="page" @after-leave="downloadBatchLeft">
          <DownloadBatchPage
            v-if="downloadBatch"
            ref="downloadBatchPage"
            :key="account.epoch + ':download:' + downloadBatch.category"
            :category="downloadBatch.category"
            :query="downloadBatch.query"
            :account="account"
            :model="downloads"
            :player="player"
            :manager="playlistManager"
            :playlists="playlists"
            @close="closeDownloadBatch()"
            @expired="music.bootstrap(false)"
          />
        </Transition>
        <Transition name="page" @after-leave="cloudBatchLeft">
          <CloudBatchPage
            v-if="cloudBatchTarget"
            ref="cloudBatchPage"
            :key="cloudBatchTarget.epoch + ':cloud-batch'"
            :account="account"
            :model="cloud"
            :player="player"
            :downloads="downloads"
            :manager="playlistManager"
            :playlists="playlists"
            :leaving="cloudBatchClosing"
            @close="closeCloudBatch()"
            @expired="music.bootstrap(false)"
            @downloaded="music.navigate({ kind: 'downloads', initialTab: 'active' })"
          />
        </Transition>
        <div
          v-show="!playlistEditor && !batchTarget && !downloadBatch && !cloudBatchTarget"
          class="content-page-previous"
        >
          <Transition
            :css="false"
            mode="out-in"
            @enter="pageTransition.enter"
            @leave="pageTransition.leave"
            @enter-cancelled="pageTransition.cancel"
            @leave-cancelled="pageTransition.cancel"
            @after-enter="mainPageEntered"
          >
            <div class="page-view" :key="pageKey">
              <LocalPage
                ref="localPageRef"
                v-if="page.kind === 'local'"
                :model="local"
                :matching="localMatching"
                :playable="player.ready.value"
                :queueable="
                  player.ready.value &&
                  !player.busy.value &&
                  !player.queueBusy.value &&
                  !player.view.value?.fm
                "
                :queue-local="player.queueLocal"
                :current-id="currentLocalId"
                @play="playLocal"
                @comments="(id, title) => openComments({ kind: 'track', id }, title)"
              />
              <div v-else-if="initializing" class="empty-state" role="status">
                <span class="spinner" />
                <p>正在连接你的音乐…</p>
              </div>
              <HistoryPage
                v-else-if="page.kind === 'history'"
                :model="recent"
                :playable="player.ready.value && !player.busy.value"
                :current-id="currentTrackId"
                :current-voice-id="currentVoiceId"
                @scope="music.replace({ kind: 'history', scope: $event })"
                @play="playHistory"
                @open="openHistory"
              />
              <SettingsPage
                :player="player"
                v-else-if="page.kind === 'settings'"
                ref="settingsPage"
                :model="settings"
                :relay="relay"
                :publication="publication"
                :account="account"
                :account-detail="page.accountDetail"
                @login="loginOpen = true"
                @accounts="accountsOpen = true"
                @downloads="music.navigate({ kind: 'downloads' })"
                @account-detail="
                  (kind) => music.navigate({ kind: 'settings', accountDetail: kind })
                "
                @back="historyLength ? music.back() : music.replace({ kind: 'settings' })"
                @expired="music.bootstrap(false)"
              />
              <ColdOfflinePage
                v-else-if="coldOfflineEligible"
                :model="coldOffline"
                @retry="music.bootstrap()"
              />
              <AccountWelcome
                v-else-if="!signedIn"
                :account="account"
                :error="error"
                @login="loginOpen = true"
                @retry="music.bootstrap()"
              />
              <MvCollection v-else-if="page.kind === 'mvs'" :model="mv" @open="openMv" />
              <CollectionsPage
                ref="collectionsPageRef"
                v-else-if="page.kind === 'collections'"
                :account="account"
                :scope="page.scope"
                :visit="page.collectionVisit ?? 0"
                :memory="collectionMemory"
                :revision="collectionRevision"
                :playable="player.ready.value && !player.busy.value"
                :queueable="
                  player.ready.value &&
                  !player.busy.value &&
                  !player.queueBusy.value &&
                  !player.view.value?.fm
                "
                :origin="player.loaded.value ? player.view.value?.origin : null"
                :playing="player.playing.value"
                @scope="music.replace({ kind: 'collections', scope: $event })"
                @open="openCatalog"
                @play-album="playAlbumCard"
                @album-action="albumAction"
                @artist-action="artistAction"
                @expired="music.bootstrap(false)"
              />
              <SearchResultsPage
                :memory="searchMemory"
                :search-id="page.kind === 'search' ? page.searchId : undefined"
                v-else-if="page.kind === 'search'"
                :account="account"
                :keyword="page.keyword"
                :category="page.category"
                :channel="page.channel"
                :pagination="page.pagination"
                :correct="page.correct"
                :rewritten-keyword="page.rewrittenKeyword"
                :liked="liked"
                :busy-likes="busyLikes"
                :playable="player.ready.value && !player.busy.value"
                :queueable="searchCollectionQueueable"
                :current-id="currentSearchSongId"
                :playback-origin="currentOrigin"
                :playback-state="playbackState"
                :current-voice-id="searchCurrentVoiceId(player.view.value)"
                @navigate="
                  (category, pagination, correct, rewrittenKeyword) =>
                    page.kind === 'search' &&
                    music.navigate({
                      ...page,
                      category,
                      pagination,
                      correct,
                      rewrittenKeyword,
                      channel: correct ? page.channel : undefined,
                    })
                "
                @search="(keyword) => music.navigate({ kind: 'search', keyword })"
                @search-original="searchOriginal"
                @reason="
                  (label, target) => music.navigate({ kind: 'search_reason', label, target })
                "
                @open="openSearchItem"
                @mv="openMv"
                @creator="(id) => music.navigate({ kind: 'playlist_creator', id })"
                @play="playSearchResults"
                @play-track="playSearchTrack"
                @play-card="playSearchCard"
                @queue-card="queueSearchCard"
                @download-track="(track) => (downloadDialog = { kind: 'start', tracks: [track] })"
                @card-comments="searchCardComments"
                @card-download="
                  (item) =>
                    item.kind === 'voice' &&
                    (downloadDialog = { kind: 'voice', tracks: [item.voice] })
                "
                @collect="(tracks) => (playlistDialog = { kind: 'tracks', tracks })"
                @more="(request) => trackActions?.show(request)"
                @like="music.like"
                @artist="(id) => openCatalog({ kind: 'artist', id })"
                @album="(id) => openCatalog({ kind: 'album', id })"
                @expired="music.bootstrap(false)"
              />
              <SearchReasonPage
                v-else-if="page.kind === 'search_reason'"
                :epoch="account.epoch"
                :label="page.label"
                :target="page.target"
                :liked="liked"
                :busy-likes="busyLikes"
                :playable="player.ready.value && !player.busy.value"
                @play="playOnlineSongs"
                @like="music.like"
                @more="(request) => trackActions?.show(request)"
                @artist="(id) => openCatalog({ kind: 'artist', id })"
                @album="(id) => openCatalog({ kind: 'album', id })"
                @expired="music.bootstrap(false)"
              />
              <CatalogPage
                v-else-if="page.kind === 'catalog' && catalogSession"
                ref="catalogDetailPage"
                :model="catalogSession"
                :account="account"
                :playable="player.ready.value"
                :play-busy="player.busy.value"
                :origin="player.loaded.value ? player.view.value?.origin : null"
                :playing="player.playing.value"
                :queueable="
                  player.ready.value &&
                  !player.busy.value &&
                  !player.queueBusy.value &&
                  !player.view.value?.fm
                "
                :download-busy="downloads.busy.value"
                :liked="liked"
                :busy-likes="busyLikes"
                :current-id="currentTrackId"
                @open="openCatalog"
                @profile="(id) => music.navigate({ kind: 'playlist_creator', id })"
                @play-album="playAlbumCard"
                @album-action="albumAction"
                @play-artist="playResourceCard({ kind: 'artist', id: $event.id })"
                @artist-action="artistAction"
                @artist-tab="
                  (tab) => {
                    if (page.kind === 'catalog') music.navigate({ ...page, artistTab: tab });
                  }
                "
                @album-tab="
                  (tab) => {
                    if (page.kind === 'catalog') music.replace({ ...page, albumTab: tab });
                  }
                "
                @back="backFromPage"
                @play="playCatalog"
                @play-tracks="playOnlineSongs"
                :queue-artist-songs="player.queueArtistSongs"
                @collect-tracks="(tracks) => (playlistDialog = { kind: 'tracks', tracks })"
                @like="music.like"
                @more="(request) => trackActions?.show(request)"
                @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
                @mv="openMv"
                @expired="music.bootstrap(false)"
              />
              <div v-else-if="page.kind === 'catalog'" class="empty-state">
                请登录后查看详情<button class="text-button" @click="loginOpen = true">
                  登录账号
                </button>
              </div>
              <MemberPage
                v-else-if="page.kind === 'member'"
                :model="member"
                :nickname="account.nickname"
                :playable="player.ready.value && !player.busy.value"
                :liked="liked"
                :busy-likes="busyLikes"
                :current-id="currentTrackId"
                @open="openMemberTarget"
                @play="playMember"
                @play-playlist="playMemberPlaylist"
                @like="music.like"
                @more="(request) => trackActions?.show(request)"
                @comments="trackComments"
                @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
              />
              <SceneFmPage
                v-else-if="page.kind === 'scene_fm'"
                :model="sceneFm"
                :playable="player.ready.value && !player.busy.value"
                @radio="openSceneRadio"
                @comments="(id, title) => openComments({ kind: 'voice', id }, title)"
                @download="(voice) => (downloadDialog = { kind: 'voice', tracks: [voice] })"
              />
              <FmPage
                v-else-if="page.kind === 'fm'"
                :model="fm"
                :player="player"
                :liked="liked"
                :like-busy="busyLikes"
                @like="music.like"
                @comments="trackComments"
                @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
              />
              <RankingsPage
                ref="rankingsPageRef"
                v-else-if="page.kind === 'rankings'"
                :model="rankings"
                :playable="player.ready.value && !player.busy.value"
                :liked="liked"
                :busy-likes="busyLikes"
                :current-id="currentTrackId"
                :current-playlist-id="currentPlaylistId"
                :playback-state="playbackState"
                @navigate="openRanking"
                @playlist="(id) => music.navigate({ kind: 'playlist', id })"
                @play-playlist="playRankingPlaylist"
                @play="playOnlineSongs"
                @play-album="playAlbum"
                @like="music.like"
                @more="(request) => trackActions?.show(request)"
                @comments="trackComments"
                @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
              />
              <TogetherPage v-else-if="page.kind === 'together'" :model="together" />
              <PodcastPage
                v-else-if="page.kind === 'podcasts'"
                :model="podcasts"
                :playable="player.ready.value && !player.busy.value"
                :current-id="currentVoiceId"
                @play="playPodcast"
                @comments="(id, title) => openComments({ kind: 'voice', id }, title)"
                @download="(tracks) => (downloadDialog = { kind: 'voice', tracks })"
              />
              <PlaylistCreatorPage
                v-else-if="page.kind === 'playlist_creator'"
                :model="playlistCreator"
                @open="(id) => music.navigate({ kind: 'playlist', id })"
              />
              <PlaylistTagBrowse
                v-else-if="page.kind === 'playlist_tag'"
                :tag="page.tag"
                :model="playlistTagBrowse"
                @open="(id) => music.navigate({ kind: 'playlist', id })"
              />
              <CommentsPage
                v-else-if="page.kind === 'comments'"
                :model="comments"
                :title="page.title"
                :playable="player.ready.value && !player.busy.value"
                :player="player.view.value"
                @play="playCommentResource"
                @open="openCommentResource"
              />
              <MvPlayer
                v-else-if="page.kind === 'mv'"
                :model="mv"
                :mini="false"
                :playable="player.ready.value"
                :song-likes="liked"
                :song-busy="busyLikes"
                :song-notice="music.likeNotice.value || music.notice.value || music.error.value"
                @related="playMvRelated"
                @related-like="music.like"
                @open="openMv"
                @expand="mv.video.value?.id && openMv(mv.video.value.id)"
                @downloads="music.navigate({ kind: 'downloads' })"
                @comments="
                  mv.detail.value &&
                  openComments(
                    { kind: 'mv', id: mv.detail.value.card.id },
                    mv.detail.value.card.title,
                  )
                "
              />
              <CloudPage
                v-else-if="page.kind === 'cloud'"
                :model="cloud"
                :player="player"
                :manager="playlistManager"
                :playlists="playlists"
                @expired="music.bootstrap(false)"
                :playable="player.ready.value"
                :current-id="
                  player.view.value?.resourceKind === 'cloud' ? currentTrackId : undefined
                "
                @batch="openCloudBatch"
                @dialog="cloudDialogOpen = $event"
                @download="
                  (tracks, revision) => (downloadDialog = { kind: 'cloud', tracks, revision })
                "
              />
              <DownloadPage
                ref="downloadPageRef"
                v-else-if="page.kind === 'downloads'"
                :model="downloads"
                :playable="player.ready.value"
                :queueable="player.ready.value && !player.busy.value && !player.view.value?.fm"
                :current-id="currentTrackId"
                @dialog="downloadDialog = $event"
                @batch="openDownloadBatch"
                @artist="(id) => openCatalog({ kind: 'artist', id })"
                @collect="(task) => (playlistDialog = { kind: 'tracks', tracks: [task.track] })"
                @comments="(task) => trackComments(task.track)"
                @next="queueDownload"
                :preparing="player.busy.value"
                @select="player.select"
                @play-mv="(task) => openMv(task.track.id, task.id)"
              />
              <DailyBatchPage
                v-else-if="page.kind === 'daily_batch'"
                :page="result"
                :liked="liked"
                :current-id="searchCurrentSongId(player.view.value)"
                :loading="loading"
                :error="error"
                :player="player"
                :queueable="canQueueDaily"
                @close="music.back()"
                @refresh="music.load()"
                @expired="music.bootstrap(false)"
                @collect="(tracks) => (playlistDialog = { kind: 'tracks', tracks })"
                @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
              />
              <PlaylistLibraryPage
                v-else-if="page.kind === 'library'"
                :playlists="playlists"
                :state="playlistLibraryState"
                :loading="music.resourcesLoading.value"
                :error="music.playlistError.value"
                :notice="playlistManager.error.value || notice"
                :operation-count="
                  playlistManager.operations.value.length +
                  playlistManager.coverOperations.value.length
                "
                :items="playlistMenu"
                :playable="player.ready.value"
                :preparing="player.busy.value"
                :current-id="currentPlaylistId"
                :playing="libraryPlaying"
                @refresh="refreshLibrary"
                @create="playlistDialog = { kind: 'create' }"
                @link="playlistDialog = { kind: 'open' }"
                @history="playlistDialog = { kind: 'history' }"
                @browse="music.navigate({ kind: 'daily' })"
                @open="(list) => music.navigate({ kind: 'playlist', id: list.id })"
                @play="playLibraryPlaylist"
                @action="playlistAction"
              />
              <template v-else>
                <PlaylistHero
                  v-if="result?.playlist"
                  :key="result.playlist.id"
                  :playlist="result.playlist"
                  :total="result.total"
                  :playable="player.ready.value && result.tracks.some((track) => !track.missing)"
                  :preparing="player.busy.value"
                  :intelligence-disabled="!intelligence.canStart.value"
                  :intelligence-active="intelligence.active.value"
                  :intelligence-busy="intelligence.busy.value"
                  :intelligence-seeded="player.view.value?.intelligence === 'seeded'"
                  @intelligence="
                    intelligence.start({
                      kind: 'likes',
                      playlist_id: result.playlist.id,
                      revision: result.revision ?? undefined,
                    })
                  "
                  :download-disabled="
                    downloads.busy.value ||
                    result.tracks.every((track) => track.missing || track.local)
                  "
                  :subscribed="playlists.some((list) => list.id === result?.playlist?.id)"
                  :busy="playlistManager.busy.value"
                  :items="playlistMenu(result.playlist, 'detail')"
                  @edit="editPlaylist(result.playlist.id)"
                  @tag="(tag) => music.navigate({ kind: 'playlist_tag', tag })"
                  @creator="(id) => music.navigate({ kind: 'playlist_creator', id })"
                  @play="play(result.tracks.findIndex((track) => !track.missing))"
                  @download="
                    downloadDialog = {
                      kind: 'start',
                      tracks: result.tracks.filter((track) => !track.missing && !track.local),
                    }
                  "
                  @subscribe="
                    playlistActions?.show(
                      result.playlist,
                      playlists.some((list) => list.id === result?.playlist?.id)
                        ? 'remove'
                        : 'subscribe',
                    )
                  "
                  @action="
                    (key) => result?.playlist && playlistAction(result.playlist, key, 'detail')
                  "
                />
                <div
                  v-else
                  v-show="!recommendationStickyVisible"
                  class="page-heading"
                  :class="{ 'daily-heading': page.kind === 'daily' }"
                >
                  <DailyHeading
                    v-if="page.kind === 'daily'"
                    :birthday="account.status === 'authenticated' && result?.daily?.birthdayToday"
                    :nickname="account.nickname"
                  />

                  <div v-else>
                    <h1
                      v-if="page.kind === 'style_recommendations' && page.seed"
                      ref="styleHomeHeading"
                      tabindex="-1"
                    >
                      {{ title }}
                    </h1>
                    <h1 v-else>{{ title }}</h1>
                    <p class="muted">
                      {{
                        result
                          ? page.kind === 'style_recommendations'
                            ? page.seed
                              ? result.styleTags?.tags.map((tag) => tag.name).join(' · ') ||
                                '来自首页的风格推荐'
                              : '定制您今日的专属好音乐'
                            : `找到 ${result.total} 首歌曲`
                          : error
                            ? '加载失败，可以重试'
                            : page.kind === 'playlist'
                              ? '正在读取歌单…'
                              : '正在寻找音乐…'
                      }}
                    </p>
                  </div>
                  <div class="page-heading-actions">
                    <StyleHomeBrowser
                      v-if="page.kind === 'style_recommendations' && !page.seed"
                      ref="styleHomeBrowser"
                      :epoch="account.epoch"
                      @open="openHomeStyle"
                      @expired="music.bootstrap(false)"
                    />
                    <StyleTagBrowser
                      v-if="page.kind === 'style_recommendations' && !page.seed"
                      :epoch="account.epoch"
                      :applied="result?.styleTags"
                      :revision="result?.revision"
                      @refreshed="music.load()"
                      @expired="music.bootstrap(false)"
                    />
                    <button
                      class="icon-button"
                      aria-label="刷新当前页面"
                      :disabled="loading"
                      @click="music.load()"
                    >
                      <Icon name="refresh" />
                    </button>
                  </div>
                </div>
                <div
                  v-if="page.kind === 'daily' && result?.daily?.cached"
                  class="notice"
                  role="status"
                  :aria-busy="loading"
                >
                  <span>暂时无法获取今日推荐，正在显示本账号上次加载的列表。</span>
                  <button
                    type="button"
                    class="text-button"
                    :aria-disabled="loading"
                    @click="retryDailyCache"
                  >
                    {{ loading ? '正在重试…' : '重试' }}
                  </button>
                </div>
                <div v-if="notice || music.likeNotice.value" class="notice" role="status">
                  {{ music.likeNotice.value || notice
                  }}<button class="text-button" @click="music.resources">刷新</button>
                </div>
                <div v-if="error" class="error-state" role="alert">
                  <span>{{ error }}</span
                  ><button class="text-button" @click="music.load()">重试</button>
                </div>
                <div
                  class="playlist-toolbar"
                  :class="{ 'playlist-content-nav': !!result?.playlist }"
                >
                  <button
                    v-if="result?.playlist"
                    type="button"
                    class="text-button playlist-section"
                    :class="{ 'playlist-current-section': playlistSongsActive }"
                    :aria-pressed="playlistSongsActive"
                    @click="showPlaylistSongs"
                  >
                    歌曲 <small>{{ result.total }}</small>
                  </button>
                  <template v-if="result?.playlist">
                    <button
                      type="button"
                      class="text-button playlist-section"
                      :class="{ 'playlist-current-section': playlistComments.active.value }"
                      aria-label="歌单评论"
                      :aria-pressed="playlistComments.active.value"
                      @click="showPlaylistComments"
                    >
                      评论
                      <small v-if="playlistComments.count.value">{{
                        playlistComments.count.value
                      }}</small>
                    </button>
                    <button
                      type="button"
                      class="text-button playlist-section"
                      :class="{ 'playlist-current-section': subscribers.open.value }"
                      :aria-pressed="subscribers.open.value"
                      @click="showPlaylistSubscribers"
                    >
                      收藏者
                      <small v-if="result.playlist.subscribedCount">{{
                        result.playlist.subscribedCount
                      }}</small>
                    </button>
                  </template>
                  <TrackScopeSearch
                    v-if="result?.playlist && playlistSongsActive"
                    v-model="playlistKeyword"
                    class="playlist-track-search"
                    label="搜索歌单歌曲"
                    clear-label="清空歌单搜索"
                  />
                  <button
                    v-if="playlistLocateIndex !== undefined"
                    type="button"
                    class="text-button playlist-locate"
                    @click="playlistTrackTable?.locate()"
                  >
                    定位当前歌曲
                  </button>
                  <button
                    v-if="
                      playlistManager.operations.value.length +
                      playlistManager.coverOperations.value.length
                    "
                    class="text-button"
                    @click="playlistDialog = { kind: 'history' }"
                  >
                    歌单操作记录 ·
                    {{
                      playlistManager.operations.value.length +
                      playlistManager.coverOperations.value.length
                    }}
                  </button>
                  <span
                    v-if="playlistManager.error.value && !playlistDialog"
                    class="form-warning"
                    role="alert"
                    >{{ playlistManager.error.value }}</span
                  >
                </div>
                <div
                  v-if="page.kind === 'daily' || (result?.tracks.length && !result.playlist)"
                  v-show="!recommendationStickyVisible"
                  class="track-actions"
                  :class="{ 'daily-track-actions': page.kind === 'daily' }"
                >
                  <button
                    class="primary"
                    :disabled="!result?.tracks.length || !player.ready.value || player.busy.value"
                    @click="play(0)"
                  >
                    <Icon name="play" />播放全部
                  </button>
                  <button
                    class="download-button"
                    aria-label="下载当前歌曲列表"
                    title="下载当前歌曲列表"
                    :disabled="
                      downloads.busy.value || !result?.tracks.some((t) => !t.missing && !t.local)
                    "
                    @click="
                      result &&
                      (downloadDialog = {
                        kind: 'start',
                        tracks: result.tracks.filter((t) => !t.missing && !t.local),
                      })
                    "
                  >
                    <Icon v-if="page.kind === 'daily'" name="download" />
                    {{ page.kind === 'daily' ? '下载' : '下载当前歌曲列表' }}
                  </button>
                  <button
                    v-if="page.kind === 'style_recommendations'"
                    class="secondary style-queue-all"
                    type="button"
                    :disabled="!result?.revision || !canQueueDaily"
                    @click="queueRecommendations"
                  >
                    <Icon name="list-add" />添加全部至播放列表
                  </button>
                  <ActionMenu
                    v-if="page.kind === 'daily'"
                    class="daily-more"
                    label="每日推荐更多操作"
                    :disabled="!result?.tracks.length || !result.revision || loading"
                    :items="dailyMenuItems"
                    @opened="dailyMenuRevision = result?.revision"
                    @select="dailyMenuAction"
                  />
                  <span v-if="player.busy.value" class="muted" role="status">正在准备播放…</span>
                  <nav v-if="page.kind === 'daily'" class="daily-links" aria-label="每日推荐入口">
                    <button
                      class="text-button daily-style-entry"
                      type="button"
                      @click="openStyleRecommendations"
                    >
                      风格推荐
                    </button>
                    <span class="daily-link-divider" aria-hidden="true"></span>
                    <button
                      class="text-button daily-history-entry"
                      type="button"
                      @click="openDailyHistory"
                    >
                      历史日推 <span class="daily-history-badge" aria-hidden="true">VIP</span>
                    </button>
                  </nav>
                </div>
                <p
                  v-if="recommendationScrollScope && dailyQueueMessage"
                  class="batch-operation-feedback"
                  :class="{ failed: dailyQueueFailed }"
                  role="status"
                >
                  {{ dailyQueueMessage }}
                </p>
                <div v-if="loading && !result" class="empty-state" role="status">
                  <span class="spinner" />
                  <p>正在加载歌曲…</p>
                </div>
                <CommentsPage
                  v-else-if="result?.playlist && playlistComments.active.value"
                  :model="playlistComments.model"
                  :title="result.playlist.name"
                  embedded
                />
                <PlaylistSubscribers
                  v-else-if="result?.playlist && subscribers.open.value"
                  :model="subscribers"
                  :restore-focus="subscriberFocus"
                  @profile="openSubscriber"
                  @restored="subscriberFocus = undefined"
                />
                <template v-else-if="result">
                  <p
                    v-if="playlistTrackSearch.positions !== null"
                    class="playlist-track-match"
                    role="status"
                  >
                    {{ playlistTrackSearch.tracks.length }} 首匹配歌曲
                    <template v-if="result.more"
                      >（已搜索 {{ result.tracks.length }} /
                      {{ result.total }} 首，可继续加载）</template
                    >
                  </p>
                  <div
                    v-if="recommendationStickyVisible"
                    class="recommendation-sticky"
                    role="group"
                    :aria-label="page.kind === 'daily' ? '每日推荐快捷操作' : '风格推荐快捷操作'"
                  >
                    <DailyHeading
                      v-if="page.kind === 'daily'"
                      compact
                      :birthday="result.daily?.birthdayToday"
                    />
                    <span v-else>风格推荐</span>
                    <button
                      class="icon-button"
                      type="button"
                      aria-label="播放全部推荐歌曲"
                      :disabled="!player.ready.value || player.busy.value || loading"
                      @click="play(0)"
                    >
                      <Icon name="play" />
                    </button>
                    <button
                      class="icon-button"
                      type="button"
                      aria-label="添加全部推荐歌曲至播放列表"
                      :disabled="!canQueueDaily"
                      @click="queueRecommendations"
                    >
                      <Icon name="list-add" />
                    </button>
                  </div>
                  <TrackTable
                    v-if="playlistTrackSearch.tracks.length"
                    :key="recommendationScrollScope"
                    ref="playlistTrackTable"
                    :locate-index="playlistLocateIndex"
                    :scroll-scope="recommendationScrollScope"
                    @scroll-edge="recommendationScrollEdge"
                    navigable
                    @artist="(id) => openCatalog({ kind: 'artist', id })"
                    @album="(id) => openCatalog({ kind: 'album', id })"
                    :tracks="playlistTrackSearch.tracks"
                    :match-keyword="
                      playlistTrackSearch.positions !== null ? playlistKeyword : undefined
                    "
                    :liked="liked"
                    :busy="busyLikes"
                    :playable="player.ready.value"
                    :current-id="currentTrackId"
                    @play="playVisibleTrack"
                    @like="music.like"
                    @more="showVisibleTrackActions"
                  />
                  <div v-else class="empty-state">
                    {{
                      playlistTrackSearch.positions === null
                        ? '这里还没有歌曲'
                        : '没有找到匹配的歌曲'
                    }}
                  </div>
                  <button
                    v-if="result.more"
                    class="load-more"
                    :disabled="loading"
                    @click="music.load(true)"
                  >
                    {{
                      loading
                        ? '正在加载…'
                        : `加载更多（已显示 ${result.tracks.length} / ${result.total}）`
                    }}
                  </button></template
                >
              </template>
            </div>
          </Transition>
        </div>
      </div>
    </main>
    <MvPlayer
      v-if="signedIn && page.kind !== 'mv' && mv.video.value?.id && mv.video.value.state !== 'idle'"
      :model="mv"
      :mini="true"
      @expand="openMv(mv.video.value.id)"
      @downloads="music.navigate({ kind: 'downloads' })"
    />
    <Transition name="lyrics-panel" @before-enter="lyricPanelEnter" @before-leave="lyricPanelLeave">
      <LyricsPanel
        v-if="player.lyricsOpen.value && currentTrackId"
        :player="player"
        @report="reportLyrics"
        @source="openPlaybackSource"
        @catalog="
          (target) => {
            player.lyricsOpen.value = false;
            openCatalog(target);
          }
        "
      />
    </Transition>
    <ModalTransition
      :restore-focus="!initializing && lyricReport.ownerEpoch.value === account.epoch"
      ><LyricReportDialog v-if="lyricReport.open.value" :model="lyricReport"
    /></ModalTransition>
    <Transition name="queue-panel" @after-leave="queueLeft">
      <QueuePanel
        v-if="player.queueOpen.value"
        :player="player"
        :authenticated="signedIn"
        :together="together"
        @collect="collectQueue"
        @login="loginOpen = true"
        @discover="discoverFromQueue"
        @together="openTogetherFromQueue"
        @cleared="together.load(false, true)"
      />
    </Transition>
    <RelayPrompt
      :model="relay"
      :blocked="
        !!(
          loginOpen ||
          accountsOpen ||
          playlistDialog ||
          playlistEditor ||
          batchTarget ||
          downloadBatch ||
          downloadDialog ||
          cloudDialogOpen ||
          player.queueOpen.value ||
          player.lyricsOpen.value
        )
      "
      :quality="player.view.value?.qualityPreferences.quality ?? 'standard'"
    />
    <WindowClosePrompt />
    <ModalTransition :fallback-focus="likeRecoveryFocus">
      <LikeRecoveryDialog
        v-if="likeRecovery.open.value"
        :key="account.epoch"
        :model="likeRecovery"
      />
    </ModalTransition>
    <PlayerBar
      :account="account"
      :player="player"
      :intelligence="intelligence"
      :liked-playlist-id="signedIn ? favorite?.id : null"
      :liked="playingLikes"
      :like-busy="playingLikeBusy"
      :downloads-allowed="signedIn"
      :together-active="together.view.value?.attached ?? false"
      @like="likePlaying"
      @retry="player.connect(account)"
      @comments="trackComments"
      @download="openCurrentDownload"
      @member="music.navigate({ kind: 'member' })"
      @login="loginOpen = true"
    />
    <ModalTransition
      :return-focus="accountFocus.returnFocus"
      :fallback-focus="accountFocus.fallbackFocus"
      @left="accountFocus.restoreIfUnowned()"
    >
      <LoginDialog
        v-if="loginOpen"
        :epoch="account.epoch"
        @close="loginOpen = false"
        @authorized="authorized"
        @accounts="
          loginOpen = false;
          accountsOpen = true;
        "
      />
    </ModalTransition>
    <ModalTransition
      :return-focus="accountFocus.returnFocus"
      :fallback-focus="accountFocus.fallbackFocus"
      @left="accountFocus.restoreIfUnowned()"
    >
      <AccountDialog
        v-if="accountsOpen"
        :account="account"
        @close="accountsOpen = false"
        @changed="authorized"
        @login="
          accountsOpen = false;
          loginOpen = true;
        "
      />
    </ModalTransition>
    <PlaylistActions
      ref="playlistActions"
      :key="`${account.epoch}:${account.status}:playlist-actions`"
      :account="account"
      :manager="playlistManager"
      :downloads="downloads"
      :player="player"
      @expired="music.bootstrap(false)"
      @started="music.navigate({ kind: 'downloads', initialTab: 'active' })"
    />
    <AlbumActions
      ref="albumActions"
      :key="`${account.epoch}:${account.status}:album-actions`"
      :account="account"
      :player="player"
      :changed="catalogMembershipChanged"
      @expired="music.bootstrap(false)"
      @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
    />
    <ArtistActions
      ref="artistActions"
      :key="`${account.epoch}:${account.status}:artist-actions`"
      :account="account"
      :changed="catalogMembershipChanged"
      @expired="music.bootstrap(false)"
    />
    <TrackActions
      ref="trackActions"
      :key="account.epoch"
      :player="player"
      @collect="(track) => (playlistDialog = { kind: 'tracks', tracks: [track] })"
      @download="(track) => (downloadDialog = { kind: 'start', tracks: [track] })"
      @comments="(track) => openComments({ kind: 'track', id: track.id }, track.name)"
      @remove="(track, playlist) => (playlistDialog = { kind: 'remove', track, playlist })"
      @manage="(track, playlist) => (playlistDialog = { kind: 'track', track, playlist })"
    />
    <ModalTransition
      :restore-focus="
        !playlistEditor && (!queueCollectionReturn || queueCollectionOwner === account.epoch)
      "
      :return-focus="() => queueCollectionReturn ?? null"
      :fallback-focus="queueCollectionFallback"
      @left="collectionLeft"
    >
      <PlaylistCollectDialog
        v-if="playlistDialog?.kind === 'tracks'"
        :key="account.epoch + ':collect'"
        :manager="playlistManager"
        :epoch="account.epoch"
        :tracks="playlistDialog.tracks"
        :selection-summary="playlistDialog.selectionSummary"
        :playlists="playlists"
        @close="playlistDialog = undefined"
        @expired="music.bootstrap(false)"
      />
      <PlaylistDialog
        v-else-if="playlistDialog"
        :key="playlistDialog.kind"
        :manager="playlistManager"
        :target="playlistDialog"
        :playlists="playlists"
        @recover-cover="(id) => editPlaylist(id, true)"
        @close="playlistDialog = undefined"
        @open="(id) => music.navigate({ kind: 'playlist', id })"
        @comments="(track) => openComments({ kind: 'track', id: track.id }, track.name)"
        @download="
          (track) => {
            playlistDialog = undefined;
            downloadDialog = { kind: 'start', tracks: [track] };
          }
        "
      />
    </ModalTransition>
    <ModalTransition :fallback-focus="downloadReturnFocus">
      <DownloadDialog
        v-if="downloadDialog"
        :model="downloads"
        :target="downloadDialog"
        @close="downloadDialog = undefined"
        @started="music.navigate({ kind: 'downloads', initialTab: 'active' })"
      />
    </ModalTransition>
  </div>
  <ContentTooltip :reset-key="account.epoch + navigationKey" />
</template>
