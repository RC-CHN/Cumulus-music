<script setup lang="ts">
import SearchBox from './components/search/SearchBox.vue';
import SearchResultsPage from './components/search/SearchResultsPage.vue';
import { createSearchViewportMemory } from './services/searchViewport';
import ContentTooltip from './components/ContentTooltip.vue';
import type { SearchItem } from './services/search';
import { searchCardIsCurrent, searchCardSelection } from './services/searchCards';
import { computed, onMounted, onUnmounted, watch, ref, nextTick } from 'vue';
import { onDesktopLyricOpen } from './adapters/desktopLyrics';
import { useLocalShortcuts } from './composables/useLocalShortcuts';
import Icon from './components/Icon.vue';
import WindowControls from './components/WindowControls.vue';
import WindowClosePrompt from './components/WindowClosePrompt.vue';
import AppearanceMenu from './components/AppearanceMenu.vue';
import PlaylistLibraryPage from './components/playlist/PlaylistLibraryPage.vue';
import AppSidebar from './components/sidebar/AppSidebar.vue';
import { libraryView } from './services/playlistLibrary';
import DailyHeading from './components/DailyHeading.vue';
import PlaylistHero from './components/playlist/PlaylistHero.vue';
import PlaylistEditorPage from './components/playlist/PlaylistEditorPage.vue';
import { playlistMenuItems, type PlaylistMenuScene } from './services/playlistMenu';
import PlaylistActions from './components/playlist/PlaylistActions.vue';
import PlaylistBatchPage from './components/playlist/PlaylistBatchPage.vue';
import TrackActions from './components/TrackActions.vue';
import ModalTransition from './components/ModalTransition.vue';
import LoginDialog from './components/LoginDialog.vue';
import AccountDialog from './components/AccountDialog.vue';
import TrackTable from './components/TrackTable.vue';
import CatalogPage from './components/CatalogPage.vue';
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
import type { CatalogResource, CatalogSelectionPage } from './services/catalog';
import PlayerBar from './components/PlayerBar.vue';
import QueuePanel from './components/QueuePanel.vue';
import LyricsPanel from './components/LyricsPanel.vue';
import PlaylistDialog from './components/PlaylistDialog.vue';
import PlaylistCollectDialog from './components/PlaylistCollectDialog.vue';
import DownloadPage from './components/DownloadPage.vue';
import DownloadBatchPage from './components/download/DownloadBatchPage.vue';
import type { BatchCategory } from './services/downloadBatch';
import type { DownloadQuery } from './services/downloads';
import DownloadDialog from './components/DownloadDialog.vue';
import CloudPage from './components/CloudPage.vue';
import MvCollection from './components/MvCollection.vue';
import MvPlayer from './components/MvPlayer.vue';
import CommentsPage from './components/CommentsPage.vue';
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
import type { RankingQuery } from './services/rankings';
import { useTogether } from './composables/useTogether';
import { usePodcasts } from './composables/usePodcasts';
import type { PodcastScope } from './services/podcast';
import type { PlaylistView, TrackView } from './services/contracts';
import { useLocal } from './composables/useLocal';
import { useLocalMatching } from './composables/useLocalMatching';
import type { LocalQuery } from './services/local';
import { useComments } from './composables/useComments';
import type { CommentResource } from './services/comments';
import { useMv } from './composables/useMv';
import { useCloud } from './composables/useCloud';
import type { CloudQuery } from './services/cloud';
import { useDownloads } from './composables/useDownloads';
import type { DownloadDialogTarget } from './services/downloads';
import { usePlaylists } from './composables/usePlaylists';
import { playlistSaved, type PlaylistDialogTarget } from './services/playlists';
import { usePlayer } from './composables/usePlayer';
import { useMusic } from './composables/useMusic';
import { useCore } from './services/core';
const core = useCore();
const searchMemory = createSearchViewportMemory();
const playlistLibraryState = ref(libraryView());
const music = useMusic(undefined, (commit) => {
  if (batchTarget.value && batchPage.value)
    batchPage.value.requestLeave(() => {
      void closeBatch(false);
      commit();
    });
  else if (downloadBatch.value && downloadBatchPage.value)
    downloadBatchPage.value.requestLeave(() => {
      closeDownloadBatch(false);
      commit();
    });
  else if (playlistEditor.value && playlistEditorPage.value)
    playlistEditorPage.value.requestLeave(() => {
      closePlaylistEditor(false);
      commit();
    });
  else commit();
});
const settings = useSettings(core, () => {
  void music.bootstrap(false);
});
watch(
  () => [music.account.value.epoch, music.account.value.status],
  () => searchMemory.connect(music.account.value),
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
    quality: player.view.value?.requestedQuality ?? 'standard',
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
      quality: player.view.value?.requestedQuality ?? 'standard',
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
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
}
function playMemberPlaylist(id: string, startId: string | null) {
  void player.select({
    kind: 'playlist',
    id,
    start_id: startId,
    revision: null,
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
}
const sceneFm = useSceneFm(core, () => {
  void music.bootstrap(false);
});
function openSceneRadio(id: string) {
  podcasts.navigate({ kind: 'radio', id, ascending: true });
  music.navigate({ kind: 'podcasts' });
}
const fm = useFm(undefined, () => {
  void music.bootstrap(false);
});
const rankings = useRankings(undefined, () => {
  void music.bootstrap(false);
});
function openRanking(query: RankingQuery) {
  if (query.kind === 'album') openCatalog({ kind: 'album', id: query.id });
  else music.navigate({ kind: 'rankings', query });
}
function openCatalog(resource: CatalogResource) {
  music.navigate({ kind: 'catalog', resource });
}
function openSearchItem(item: SearchItem) {
  if (item.kind === 'artist') openCatalog({ kind: 'artist', id: item.artist.id });
  else if (item.kind === 'album') openCatalog({ kind: 'album', id: item.album.id });
  else if (item.kind === 'playlist') music.navigate({ kind: 'playlist', id: item.playlist.id });
  else if (item.kind === 'podcast') openSceneRadio(item.podcast.id);
  else if (item.kind === 'voice') {
    if (item.voice.radioId) openSceneRadio(item.voice.radioId);
    else music.notice.value = '这条声音暂未提供所属播客';
  } else if (item.kind === 'mv') {
    if (item.mv.mvId) openMv(item.mv.mvId);
    else music.notice.value = '此视频暂不可打开';
  }
}
function playSearchCard(item: SearchItem) {
  const view = player.view.value;
  if (
    searchCardIsCurrent(item, view?.origin, view?.track?.voiceId ?? undefined) &&
    player.loaded.value
  ) {
    void player.action({ kind: 'pause', paused: player.playing.value });
  } else {
    const quality = view?.requestedQuality ?? 'standard';
    void player.prepareSelection((epoch) => searchCardSelection(core, epoch, item, quality));
  }
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
function playCatalog(pages: CatalogSelectionPage[], index: number) {
  void player.select({
    kind: 'catalog',
    pages,
    index,
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
}
function playRankingPlaylist(id: string) {
  void player.select({
    kind: 'playlist',
    id,
    start_id: null,
    revision: null,
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
}
function playRankingSongs(tracks: TrackView[], index: number) {
  if (!tracks[index] || tracks[index].missing) return;
  void player.select({
    kind: 'songs',
    ids: tracks.map((t) => t.id),
    index,
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
}
function playAlbum(id: string, revision: string, startId: string | null) {
  void player.select({
    kind: 'album',
    id,
    revision,
    start_id: startId,
    quality: player.view.value?.requestedQuality ?? 'standard',
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
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
  if (player.view.value?.track?.id === songId && !player.error.value && !player.lyricsOpen.value)
    player.toggleLyrics();
}
const player = usePlayer(undefined, () => {
  void music.bootstrap(false);
});
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
function backFromPage() {
  if (batchTarget.value) batchPage.value?.requestLeave();
  else if (downloadBatch.value) downloadBatchPage.value?.requestLeave();
  else if (playlistEditor.value) playlistEditorPage.value?.requestLeave();
  else music.back();
}
const downloadDialog = ref<DownloadDialogTarget>();
function downloadReturnFocus() {
  return (
    document.querySelector<HTMLElement>('.download-mv-cover[tabindex="0"]') ??
    document.querySelector<HTMLElement>('.download-tabs [aria-selected="true"]')
  );
}
const downloads = useDownloads(undefined, () => {
  void music.bootstrap(false);
});
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
function playCloud(query: CloudQuery, revision: string, startId: string | null) {
  void player.select({
    kind: 'cloud',
    query,
    revision,
    start_id: startId,
    quality: player.view.value?.requestedQuality ?? 'standard',
  });
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
const loginOpen = ref(false);
const accountsOpen = ref(false);
const favorite = computed(() => playlists.value.find((p) => p.favorite));
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
const currentPlaylistId = computed(() =>
  player.view.value?.origin?.kind === 'playlist' ? player.view.value.origin.id : undefined,
);
const libraryPlaying = computed(() => player.view.value?.state === 'playing');
function refreshLibrary() {
  notice.value = '';
  void music.resources();
}
function playLibraryPlaylist(list: PlaylistView) {
  if (currentPlaylistId.value === list.id)
    void player.action({ kind: 'pause', paused: libraryPlaying.value });
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
const pageKey = computed(() =>
  JSON.stringify(
    page.value.kind === 'search' ? { kind: 'search', keyword: page.value.keyword } : page.value,
  ),
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
let mounted = true;
onMounted(async () => {
  try {
    const stop = await core.onAccountChanged((next) => {
      if (
        next.epoch !== account.value.epoch ||
        next.status !== account.value.status ||
        next.canSignOut !== account.value.canSignOut
      )
        void music.bootstrap(false);
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
  if (mounted) await music.bootstrap();
});
onUnmounted(() => {
  mounted = false;
  accountListener?.();
  nativeLikeListener?.();
  desktopLyricListener?.();
});
watch(
  () => [account.value.epoch, account.value.status],
  () => {
    playlistDialog.value = undefined;
    playlistEditor.value = undefined;
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
    if (page.value.kind === 'rankings') void rankings.open(page.value.query);
    comments.connect(account.value);
    localMatching.connect(account.value);
    if (page.value.kind === 'mv') void loadMv(page.value.id, page.value.taskId);
    if (page.value.kind === 'comments') void comments.open(page.value.resource);
  },
);
watch(
  () => [account.value.epoch, account.value.status, initializing.value],
  () => {
    if (!initializing.value) void player.connect(account.value);
  },
);
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
    if (page.value.kind === 'settings') void settings.open();
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
    if (value.kind === 'rankings') void rankings.open(value.query);
    if (value.kind === 'history') void recent.open(value.scope);
    if (value.kind === 'settings') void settings.open();
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
  () => [page.value.kind, !!downloadDialog.value, !!playlistActions.value?.downloadOpen],
  () =>
    downloads.show(
      page.value.kind === 'downloads' ||
        !!downloadDialog.value ||
        !!playlistActions.value?.downloadOpen,
    ),
);
function play(index = 0) {
  const tracks = result.value?.tracks;
  if (!tracks?.[index] || tracks[index].missing) return;
  const quality = player.view.value?.requestedQuality ?? 'standard';
  if (page.value.kind === 'playlist')
    void player.select({
      kind: 'playlist',
      id: page.value.id,
      start_id: tracks[index].id,
      revision: result.value?.revision ?? null,
      quality,
    });
  else void player.select({ kind: 'songs', ids: tracks.map((t) => t.id), index, quality });
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
  if (player.ready.value && player.view.value?.queueLen && !player.busy.value) {
    event.preventDefault();
    void player.action(player.playing.value ? { kind: 'pause', paused: true } : { kind: 'play' });
  }
}
onMounted(() => {
  window.addEventListener('keydown', keyboard);
  void shortcutKeys.connect(account.value.epoch);
});
onUnmounted(() => {
  window.removeEventListener('keydown', keyboard);
  shortcutKeys.dispose();
});
</script>
<template>
  <div class="app-shell">
    <AppSidebar
      :key="`${account.epoch}:${account.status}`"
      :page="page"
      :playlists="playlists"
      :history-kind="recent.kind.value"
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
        :disabled="!historyLength && !playlistEditor && !batchTarget && !downloadBatch"
        @click="backFromPage"
      >
        <Icon name="back" />
      </button>
      <SearchBox
        :account="account"
        :page-key="JSON.stringify(page)"
        :keyword="page.kind === 'search' ? page.keyword : undefined"
        @search="(word) => music.navigate({ kind: 'search', keyword: word })"
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
            @downloaded="
              downloads.filter({ tab: 'active' });
              music.navigate({ kind: 'downloads' });
            "
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
        <div
          v-show="!playlistEditor && !batchTarget && !downloadBatch"
          class="content-page-previous"
        >
          <Transition name="page" mode="out-in">
            <div class="page-view" :key="pageKey">
              <div v-if="initializing" class="empty-state" role="status">
                <span class="spinner" />
                <p>正在连接你的音乐…</p>
              </div>
              <LocalPage
                v-else-if="page.kind === 'local'"
                :model="local"
                :matching="localMatching"
                :playable="player.ready.value"
                :current-id="currentLocalId"
                @play="playLocal"
                @comments="(id, title) => openComments({ kind: 'track', id }, title)"
              />
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
                :model="settings"
                :relay="relay"
                :publication="publication"
                :account="account"
                @login="loginOpen = true"
                @accounts="accountsOpen = true"
                @downloads="music.navigate({ kind: 'downloads' })"
              />
              <div v-else-if="!signedIn" class="empty-state welcome">
                <div class="welcome-note">♪</div>
                <h1>
                  {{
                    account.loginNotice?.title ||
                    (account.status === 'expired' ? '登录已失效' : '让喜欢的音乐，陪在身边')
                  }}
                </h1>
                <p aria-live="polite">
                  {{
                    account.loginNotice?.message ||
                    error ||
                    (account.status === 'expired'
                      ? '请重新登录后继续管理歌单、查看推荐和使用账号资源。'
                      : '登录网易云音乐，找回你的歌单与每日推荐')
                  }}
                </p>
                <button class="primary" @click="loginOpen = true">
                  {{
                    account.loginNotice || account.status === 'expired' ? '重新登录' : '登录账号'
                  }}</button
                ><button
                  v-if="error && !account.loginNotice"
                  class="text-button"
                  @click="music.bootstrap()"
                >
                  重新连接
                </button>
              </div>
              <MvCollection v-else-if="page.kind === 'mvs'" :model="mv" @open="openMv" />
              <CollectionsPage
                v-else-if="page.kind === 'collections'"
                :account="account"
                :scope="page.scope"
                @scope="music.replace({ kind: 'collections', scope: $event })"
                @open="openCatalog"
                @expired="music.bootstrap(false)"
              />
              <SearchResultsPage
                :memory="searchMemory"
                :search-id="page.kind === 'search' ? page.searchId : undefined"
                v-else-if="page.kind === 'search'"
                :account="account"
                :keyword="page.keyword"
                :category="page.category"
                :offset="page.offset"
                :correct="page.correct"
                :liked="liked"
                :busy-likes="busyLikes"
                :playable="player.ready.value && !player.busy.value"
                :current-id="currentTrackId"
                :playback-origin="currentOrigin"
                :playback-state="playbackState"
                :current-voice-id="currentVoiceId ?? undefined"
                @navigate="
                  (category, offset, correct) =>
                    page.kind === 'search' && music.navigate({ ...page, category, offset, correct })
                "
                @search="(keyword) => music.navigate({ kind: 'search', keyword })"
                @open="openSearchItem"
                @play="playRankingSongs"
                @play-card="playSearchCard"
                @toggle-playback="player.action({ kind: 'pause', paused: player.playing.value })"
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
              <CatalogPage
                v-else-if="page.kind === 'catalog'"
                :resource="page.resource"
                :account="account"
                :playable="player.ready.value"
                :play-busy="player.busy.value"
                :download-busy="downloads.busy.value"
                :liked="liked"
                :busy-likes="busyLikes"
                :current-id="currentTrackId"
                @open="openCatalog"
                @play="playCatalog"
                @like="music.like"
                @more="(request) => trackActions?.show(request)"
                @download="(tracks) => (downloadDialog = { kind: 'start', tracks })"
                @mv="openMv"
                @comments="(id, title) => openComments({ kind: 'album', id }, title)"
                @expired="music.bootstrap(false)"
              />
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
                v-else-if="page.kind === 'rankings'"
                :model="rankings"
                :playable="player.ready.value && !player.busy.value"
                :liked="liked"
                :busy-likes="busyLikes"
                :current-id="currentTrackId"
                @navigate="openRanking"
                @playlist="(id) => music.navigate({ kind: 'playlist', id })"
                @play-playlist="playRankingPlaylist"
                @play="playRankingSongs"
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
              <CommentsPage
                v-else-if="page.kind === 'comments'"
                :model="comments"
                :title="page.title"
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
                :playable="player.ready.value"
                :current-id="currentTrackId"
                @play="playCloud"
                @dialog="cloudDialogOpen = $event"
                @download="
                  (tracks, revision) => (downloadDialog = { kind: 'cloud', tracks, revision })
                "
              />
              <DownloadPage
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
                  :download-disabled="
                    downloads.busy.value ||
                    result.tracks.every((track) => track.missing || track.local)
                  "
                  :subscribed="playlists.some((list) => list.id === result?.playlist?.id)"
                  :busy="playlistManager.busy.value"
                  :items="playlistMenu(result.playlist, 'detail')"
                  @edit="editPlaylist(result.playlist.id)"
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
                  class="page-heading"
                  :class="{ 'daily-heading': page.kind === 'daily' }"
                >
                  <DailyHeading v-if="page.kind === 'daily'" />

                  <div v-else>
                    <h1>{{ title }}</h1>
                    <p class="muted">
                      {{
                        result
                          ? `找到 ${result.total} 首歌曲`
                          : error
                            ? '加载失败，可以重试'
                            : page.kind === 'playlist'
                              ? '正在读取歌单…'
                              : '正在寻找音乐…'
                      }}
                    </p>
                  </div>
                  <button
                    class="icon-button"
                    aria-label="刷新当前页面"
                    :disabled="loading"
                    @click="music.load()"
                  >
                    <Icon name="refresh" />
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
                  <span
                    v-if="result?.playlist"
                    class="playlist-current-section"
                    aria-current="page"
                  >
                    歌曲 <small>{{ result.total }}</small>
                  </span>
                  <template v-if="result?.playlist">
                    <button
                      class="text-button"
                      aria-label="歌单评论"
                      @click="
                        openComments(
                          { kind: 'playlist', id: result.playlist.id },
                          result.playlist.name,
                        )
                      "
                    >
                      评论
                    </button>
                  </template>
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
                <div v-if="loading && !result" class="empty-state" role="status">
                  <span class="spinner" />
                  <p>正在加载歌曲…</p>
                </div>
                <template v-else-if="result">
                  <div v-if="result.tracks.length && !result.playlist" class="track-actions">
                    <button
                      class="primary"
                      :disabled="!player.ready.value || player.busy.value"
                      @click="play(0)"
                    >
                      <Icon name="play" />{{ '播放全部' }}</button
                    ><span v-if="player.busy.value" class="muted">正在准备播放…</span>
                    <button
                      class="download-button"
                      aria-label="下载当前歌曲列表"
                      title="下载当前歌曲列表"
                      :disabled="
                        downloads.busy.value || result.tracks.every((t) => t.missing || t.local)
                      "
                      @click="
                        downloadDialog = {
                          kind: 'start',
                          tracks: result.tracks.filter((t) => !t.missing && !t.local),
                        }
                      "
                    >
                      <Icon v-if="page.kind === 'daily'" name="download" />
                      {{ page.kind === 'daily' ? '下载' : '下载当前歌曲列表' }}
                    </button>
                  </div>
                  <TrackTable
                    v-if="result.tracks.length"
                    navigable
                    @artist="(id) => openCatalog({ kind: 'artist', id })"
                    @album="(id) => openCatalog({ kind: 'album', id })"
                    :tracks="result.tracks"
                    :liked="liked"
                    :busy="busyLikes"
                    :playable="player.ready.value"
                    :current-id="currentTrackId"
                    @play="play"
                    @like="music.like"
                    @more="(request) => trackActions?.show(request, result?.playlist ?? undefined)"
                  />
                  <div v-else class="empty-state">这里还没有歌曲</div>
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
    <Transition name="lyrics-panel">
      <LyricsPanel v-if="player.lyricsOpen.value && currentTrackId" :player="player" />
    </Transition>
    <Transition name="queue-panel">
      <QueuePanel v-if="player.queueOpen.value" :player="player" />
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
      :quality="player.view.value?.requestedQuality ?? 'standard'"
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
      :player="player"
      :liked="playingLikes"
      :like-busy="playingLikeBusy"
      @like="likePlaying"
      @retry="player.connect(account)"
      @comments="trackComments"
    />
    <ModalTransition>
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
    <ModalTransition>
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
      @started="
        downloads.filter({ tab: 'active' });
        music.navigate({ kind: 'downloads' });
      "
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
    <ModalTransition :restore-focus="!playlistEditor">
      <PlaylistCollectDialog
        v-if="playlistDialog?.kind === 'tracks'"
        :key="account.epoch + ':collect'"
        :manager="playlistManager"
        :epoch="account.epoch"
        :tracks="playlistDialog.tracks"
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
        @started="
          downloads.filter({ tab: 'active' });
          music.navigate({ kind: 'downloads' });
        "
      />
    </ModalTransition>
  </div>
  <ContentTooltip :reset-key="account.epoch + JSON.stringify(page)" />
</template>
