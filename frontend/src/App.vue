<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch, ref } from 'vue';
import { useLocalShortcuts } from './composables/useLocalShortcuts';
import Icon from './components/Icon.vue';
import WindowControls from './components/WindowControls.vue';
import WindowClosePrompt from './components/WindowClosePrompt.vue';
import AppearanceMenu from './components/AppearanceMenu.vue';
import PlaylistCard from './components/PlaylistCard.vue';
import type { MenuItem } from './services/menu';
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
import DownloadPage from './components/DownloadPage.vue';
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
const music = useMusic();
const settings = useSettings(core, () => {
  void music.bootstrap(false);
});
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
const playlistDialog = ref<PlaylistDialogTarget>();
const downloadDialog = ref<DownloadDialogTarget>();
const downloads = useDownloads(undefined, () => {
  void music.bootstrap(false);
});
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
    if (playlistSaved(change.phase) && change.kind === 'create' && change.playlistId) {
      music.navigate({ kind: 'playlist', id: change.playlistId });
    } else if (music.page.value.kind === 'playlist') {
      if (
        playlistSaved(change.phase) &&
        change.kind === 'delete' &&
        music.page.value.id === change.playlistId
      )
        music.navigate({ kind: 'library' });
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
const keyword = ref('');
const favorite = computed(() => playlists.value.find((p) => p.favorite));
const own = computed(() => playlists.value.filter((p) => p.owned && !p.favorite));
const collected = computed(() => playlists.value.filter((p) => !p.owned));
function playlistMenu(list: PlaylistView): MenuItem[] {
  if (list.favorite) return [];
  const group = list.owned ? own.value : collected.value;
  const index = group.findIndex((p) => p.id === list.id);
  return [
    ...(list.owned ? [{ key: 'edit', label: '编辑歌单', icon: 'edit' }] : []),
    {
      key: 'up',
      label: '上移歌单',
      icon: 'up',
      disabled: playlistManager.busy.value || index <= 0,
    },
    {
      key: 'down',
      label: '下移歌单',
      icon: 'down',
      disabled: playlistManager.busy.value || index >= group.length - 1,
    },
  ];
}
function playlistAction(list: PlaylistView, key: string) {
  if (key === 'edit') playlistDialog.value = { kind: 'edit', id: list.id };
  else if (key === 'up' || key === 'down')
    void playlistManager.move(list.id, list.owned, key === 'up' ? -1 : 1, favorite.value?.id);
}
watch(
  () => player.view.value?.track?.voiceId,
  (voice) => {
    if (voice && !podcasts.likesKnown.value && !podcasts.loading.value) void podcasts.state(true);
  },
);
// Stable scalar dependencies keep playback clock ticks out of the page tree.
const currentTrackId = computed(() => player.view.value?.track?.id);
const currentLocalId = computed(() =>
  player.view.value?.track?.local ? currentTrackId.value : undefined,
);
const currentVoiceId = computed(() => player.view.value?.track?.voiceId);
const pageKey = computed(() => JSON.stringify(page.value));
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
function search() {
  if (keyword.value.trim()) music.navigate({ kind: 'search', keyword: keyword.value.trim() });
}
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
  if (mounted) await music.bootstrap();
});
onUnmounted(() => {
  mounted = false;
  accountListener?.();
  nativeLikeListener?.();
});
watch(
  () => [account.value.epoch, account.value.status],
  () => {
    playlistDialog.value = undefined;
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
    if (page.value.kind === 'mv') void mv.open(page.value.id, page.value.taskId);
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
    if (value.kind === 'mv') void mv.open(value.id, value.taskId);
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
  () => [page.value.kind, !!downloadDialog.value],
  () => downloads.show(page.value.kind === 'downloads' || !!downloadDialog.value),
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
    <aside class="sidebar" aria-label="主导航">
      <button class="brand" @click="music.navigate({ kind: 'daily' })">
        <span class="brand-mark">C</span><span>Cumulus<small>云间音乐</small></span>
      </button>
      <div class="sidebar-scroll">
        <nav class="nav-main">
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'member' }"
            @click="music.navigate({ kind: 'member' })"
          >
            <Icon name="star" />会员中心
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'fm' }"
            @click="music.navigate({ kind: 'fm' })"
          >
            <Icon name="music" /><span>私人漫游</span>
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'rankings' }"
            @click="openRanking({ kind: 'home' })"
          >
            <Icon name="list" />排行榜
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'together' }"
            @click="music.navigate({ kind: 'together' })"
          >
            <Icon name="headphones" />一起听
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'daily' }"
            @click="music.navigate({ kind: 'daily' })"
          >
            <Icon name="calendar" />每日推荐
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'library' }"
            @click="music.navigate({ kind: 'library' })"
          >
            <Icon name="grid" />我的歌单
          </button>
          <button
            v-if="favorite"
            class="nav-item"
            :class="{ selected: page.kind === 'playlist' && page.id === favorite.id }"
            @click="music.navigate({ kind: 'playlist', id: favorite.id })"
          >
            <Icon name="heart" />我喜欢的音乐
          </button>
        </nav>
        <button
          class="nav-item"
          :class="{ selected: page.kind === 'downloads' }"
          @click="music.navigate({ kind: 'downloads' })"
        >
          <Icon name="down" />下载管理
        </button>
        <button
          class="nav-item"
          :class="{ selected: page.kind === 'history' }"
          @click="music.navigate({ kind: 'history', scope: recent.kind.value })"
        >
          <Icon name="history" />最近播放
        </button>
        <button
          class="nav-item"
          :class="{ selected: page.kind === 'local' }"
          @click="music.navigate({ kind: 'local' })"
        >
          <Icon name="music" />本地音乐
        </button>
        <button
          class="nav-item"
          :class="{ selected: page.kind === 'cloud' }"
          @click="music.navigate({ kind: 'cloud' })"
        >
          <Icon name="cloud" />音乐云盘
        </button>
        <div class="sidebar-lists">
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'scene_fm' }"
            @click="music.navigate({ kind: 'scene_fm' })"
          >
            <Icon name="music" />随心听 FM
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'podcasts' }"
            @click="music.navigate({ kind: 'podcasts' })"
          >
            <Icon name="music" />我的播客
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'mvs' || page.kind === 'mv' }"
            @click="music.navigate({ kind: 'mvs' })"
          >
            <Icon name="video" />收藏的 MV
          </button>
          <button
            class="nav-item"
            :class="{ selected: page.kind === 'collections' }"
            @click="music.navigate({ kind: 'collections', scope: 'saved_albums' })"
          >
            <Icon name="star" />专辑与关注
          </button>
          <template
            v-for="group in [
              { title: '创建的歌单', items: own },
              { title: '收藏的歌单', items: collected },
            ]"
            :key="group.title"
          >
            <h2 class="nav-label">
              {{ group.title }} <span>{{ group.items.length }}</span>
            </h2>
            <button
              v-for="list in group.items"
              :key="list.id"
              class="nav-item playlist-nav"
              :class="{ active: page.kind === 'playlist' && page.id === list.id }"
              :title="list.name"
              @click="music.navigate({ kind: 'playlist', id: list.id })"
            >
              <Icon :name="list.private ? 'lock' : 'music'" /><span class="ellipsis">{{
                list.name
              }}</span>
            </button>
          </template>
          <button v-if="libraryError" class="text-button" @click="music.resources">
            重新加载歌单
          </button>
        </div>
      </div>
      <div class="sidebar-caption">Cumulus 社区客户端</div>
    </aside>
    <header class="topbar" data-tauri-drag-region>
      <button
        class="back-button icon-button"
        aria-label="返回上一页"
        :disabled="!historyLength"
        @click="music.back"
      >
        <Icon name="back" />
      </button>
      <form class="searchbox" role="search" @submit.prevent="search">
        <Icon name="search" /><input
          v-model="keyword"
          aria-label="搜索音乐"
          placeholder="搜索音乐、寻找好声音"
          maxlength="256"
          :disabled="!signedIn"
        /><button
          v-if="keyword"
          type="button"
          class="icon-button"
          aria-label="清空搜索"
          @click="keyword = ''"
        >
          <Icon name="close" />
        </button>
      </form>
      <div class="header-spacer" data-tauri-drag-region />
      <button
        class="account-button"
        :disabled="initializing"
        @click="signedIn ? (accountsOpen = true) : (loginOpen = true)"
      >
        <img
          v-if="account.avatarUrl"
          :src="account.avatarUrl"
          alt=""
          referrerpolicy="no-referrer"
        /><span v-else class="avatar"><Icon name="user" /></span
        ><span>{{ signedIn ? account.nickname : '点击登录' }}</span>
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
            <h1>{{ account.loginNotice?.title || '让喜欢的音乐，陪在身边' }}</h1>
            <p aria-live="polite">
              {{
                account.loginNotice?.message || error || '登录网易云音乐，找回你的歌单与每日推荐'
              }}
            </p>
            <button class="primary" @click="loginOpen = true">
              {{ account.loginNotice ? '重新登录' : '登录账号' }}</button
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
            @more="(track) => (playlistDialog = { kind: 'track', track })"
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
              openComments({ kind: 'mv', id: mv.detail.value.card.id }, mv.detail.value.card.title)
            "
          />
          <CloudPage
            v-else-if="page.kind === 'cloud'"
            :model="cloud"
            :playable="player.ready.value"
            :current-id="currentTrackId"
            @play="playCloud"
            @dialog="cloudDialogOpen = $event"
            @download="(tracks, revision) => (downloadDialog = { kind: 'cloud', tracks, revision })"
          />
          <DownloadPage
            v-else-if="page.kind === 'downloads'"
            :model="downloads"
            :playable="player.ready.value"
            :current-id="currentTrackId"
            @dialog="downloadDialog = $event"
            @play="player.downloads"
            @play-mv="(task) => openMv(task.track.id, task.id)"
          />
          <template v-else>
            <div v-if="result?.playlist" class="playlist-hero">
              <img
                v-if="result.playlist.coverUrl"
                :src="result.playlist.coverUrl"
                alt="歌单封面"
                referrerpolicy="no-referrer"
              />
              <div v-else class="hero-art"><Icon name="music" /></div>
              <div class="hero-copy">
                <span class="eyebrow">{{ result.playlist.private ? '私密歌单' : '歌单' }}</span>
                <div class="playlist-title">
                  <h1 :title="title">{{ title }}</h1>
                  <button
                    v-if="result.playlist.owned && !result.playlist.favorite"
                    class="icon-button playlist-edit"
                    title="编辑歌单"
                    aria-label="编辑歌单"
                    @click="playlistDialog = { kind: 'edit', id: result.playlist.id }"
                  >
                    <Icon name="edit" /><span class="sr-only">编辑歌单</span>
                  </button>
                </div>
                <p class="muted">{{ result.playlist.creator }} · {{ result.total }} 首歌曲</p>
                <p class="playlist-description" :title="result.playlist.description">
                  {{ result.playlist.description || '音乐，让每一天都值得收藏。' }}
                </p>
              </div>
            </div>
            <div v-else class="page-heading">
              <div>
                <h1>{{ title }}</h1>
                <p class="muted">
                  {{
                    page.kind === 'daily'
                      ? '根据你的音乐口味，每天更新'
                      : page.kind === 'library'
                        ? `${playlists.length} 个歌单，收藏每一刻心情`
                        : result
                          ? `找到 ${result.total} 首歌曲`
                          : '正在寻找音乐…'
                  }}
                </p>
              </div>
              <button
                class="icon-button"
                aria-label="刷新当前页面"
                :disabled="loading"
                @click="page.kind === 'library' ? music.resources() : music.load()"
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
            <div class="playlist-toolbar">
              <button
                v-if="page.kind === 'library'"
                class="primary"
                @click="playlistDialog = { kind: 'create' }"
              >
                新建歌单
              </button>
              <button
                v-if="page.kind === 'library'"
                class="text-button"
                @click="playlistDialog = { kind: 'open' }"
              >
                打开歌单链接
              </button>
              <template v-if="result?.playlist">
                <button
                  class="text-button"
                  @click="
                    openComments({ kind: 'playlist', id: result.playlist.id }, result.playlist.name)
                  "
                >
                  歌单评论
                </button>
                <button
                  v-if="!result.playlist.owned"
                  class="text-button"
                  :disabled="playlistManager.busy.value"
                  @click="
                    playlistManager.mutate({
                      kind: 'subscribe',
                      id: result.playlist.id,
                      subscribed: !playlists.some((p) => p.id === result?.playlist?.id),
                    })
                  "
                >
                  {{
                    playlists.some((p) => p.id === result?.playlist?.id)
                      ? '取消收藏歌单'
                      : '收藏歌单'
                  }}
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
            <div v-if="page.kind === 'library'" class="playlist-grid">
              <PlaylistCard
                v-for="list in playlists"
                :key="list.id"
                :playlist="list"
                :items="playlistMenu(list)"
                @open="music.navigate({ kind: 'playlist', id: list.id })"
                @action="playlistAction(list, $event)"
              />
            </div>
            <div v-else-if="loading && !result" class="empty-state" role="status">
              <span class="spinner" />
              <p>正在加载歌曲…</p>
            </div>
            <template v-else-if="result">
              <div v-if="result.tracks.length" class="track-actions">
                <button
                  class="primary"
                  :disabled="!player.ready.value || player.busy.value"
                  @click="play(0)"
                >
                  <Icon name="play" />{{
                    page.kind === 'search' ? '播放当前结果' : '播放全部'
                  }}</button
                ><span v-if="player.busy.value" class="muted">正在准备播放…</span>
                <button
                  class="download-button"
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
                  下载当前歌曲列表
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
                @more="
                  (track) =>
                    (playlistDialog = {
                      kind: 'track',
                      track,
                      playlist: result?.playlist ?? undefined,
                    })
                "
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
    <ModalTransition>
      <PlaylistDialog
        v-if="playlistDialog"
        :key="playlistDialog.kind === 'edit' ? 'edit:' + playlistDialog.id : playlistDialog.kind"
        :manager="playlistManager"
        :target="playlistDialog"
        :playlists="playlists"
        :account="account"
        @cover-changed="
          async () => {
            await music.resources();
            if (page.kind === 'playlist') await music.load();
          }
        "
        @expired="music.bootstrap(false)"
        @recover-cover="(id) => (playlistDialog = { kind: 'edit', id, cover: true })"
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
    <ModalTransition>
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
</template>
