<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';
import type { SearchCategory, SearchItem } from '../../services/search';
import { podcastProgramPublicLink, type PodcastSearchFollow } from '../../services/podcast';
import type { PlaylistView } from '../../services/contracts';
import {
  playlistLinkMenuItem,
  playlistSubscribeMenuItem,
  type PlaylistLinkAction,
} from '../../services/playlistMenu';
import { artworkAt } from '../../services/artwork';
import { time, type SearchPodcastRowGesture } from '../../services/player';
import { playlistCoverCount, playlistTableCount } from '../../services/playlistHeader';
import Icon from '../Icon.vue';
import SearchMatchText from './SearchMatchText.vue';
import SearchArtistCredits from './SearchArtistCredits.vue';
import ActionMenu from '../ActionMenu.vue';
import type { MenuItem } from '../../services/menu';
import { artistCardMenu, type ArtistCardRequest } from '../../services/artistCards';
import { albumCardDate, albumLink, type AlbumCardRequest } from '../../services/catalogCards';
import {
  searchAlbumAlias,
  searchAlbumCredits,
  searchCardCanPlay,
  searchCardCanQueue,
  searchCardKey,
  searchMvPlaybackTarget,
  searchPlaylistCreator,
  searchPodcastCardSelection,
  searchPodcastRowSelection,
  searchVideoCredits,
  searchVideoCoverStats,
  type SearchPendingVoiceToggle,
  type SearchQueueRequest,
  type SearchVoiceLinkRequest,
} from '../../services/searchCards';
import { usePlaybackTriggerFocus } from '../../composables/usePlaybackTriggerFocus';
import { useCore } from '../../services/core';
import { failure } from '../../services/contracts';
import type { SearchVoiceMenu, SearchVoiceMenuTarget } from '../../services/search';
const props = defineProps<{
  item: SearchItem;
  category: SearchCategory;
  tableRow?: { index: number; number: number };
  matchKeyword: string;
  playable: boolean;
  queueable: boolean;
  podcastNextEnabled?: boolean;
  voiceNext?: 'ready' | 'disabled';
  voiceLinks?: boolean;
  voiceMenuEpoch?: string;
  playlistActionBusy: boolean;
  playlistKnown?: boolean;
  albumSubscribed?: boolean | null;
  podcastFollow?: PodcastSearchFollow;
  current: boolean;
  pendingVoiceToggle?: SearchPendingVoiceToggle;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [item: SearchItem];
  creator: [id: string];
  artist: [id: string];
  artistAction: [request: ArtistCardRequest];
  albumAction: [request: AlbumCardRequest];
  podcastFollow: [state: PodcastSearchFollow];
  play: [item: SearchItem, gesture?: SearchPodcastRowGesture];
  togglePendingVoice: [target: SearchPendingVoiceToggle, anchor: HTMLButtonElement];
  queue: [request: SearchQueueRequest];
  queueVoice: [id: string];
  voiceLink: [request: SearchVoiceLinkRequest];
  voiceMenuError: [message: string];
  voiceMenuPlay: [target: SearchVoiceMenuTarget];
  playlistDownload: [playlist: PlaylistView];
  playlistLink: [playlist: PlaylistView, action: PlaylistLinkAction];
  playlistSubscribe: [playlist: PlaylistView];
  comments: [item: SearchItem];
  download: [item: SearchItem];
}>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const element = ref<HTMLElement>();
const core = useCore();
const ordinaryVoiceMenu = computed(() => props.voiceLinks && props.item.kind === 'voice');
const voiceMenu = ref<SearchVoiceMenu>();
const voiceMenuTarget = computed<SearchVoiceMenuTarget | undefined>(() =>
  props.voiceMenuEpoch && props.item.kind === 'voice' && props.item.menuReceipt
    ? { receipt: props.item.menuReceipt, voiceId: props.item.voice.id }
    : undefined,
);
function voiceMenuLive(item: SearchItem, epoch: string, target: SearchVoiceMenuTarget) {
  return (
    item === props.item &&
    epoch === props.voiceMenuEpoch &&
    target.receipt === voiceMenuTarget.value?.receipt &&
    element.value?.isConnected &&
    !element.value.closest('[inert],.page-leave-active,.search-results-leave-active')
  );
}
async function prepareVoiceMenu(signal: AbortSignal) {
  const { item, voiceMenuEpoch: epoch } = props;
  const target = voiceMenuTarget.value;
  voiceMenu.value = undefined;
  if (!target || !epoch || !voiceMenuLive(item, epoch, target)) return false;
  try {
    const response = await core.searchVoiceMenu(epoch, target);
    if (signal.aborted || response.epoch !== epoch || !voiceMenuLive(item, epoch, target))
      return false;
    voiceMenu.value = response.value;
    return true;
  } catch {
    return false;
  }
}
async function openVoiceFolder() {
  const { item, voiceMenuEpoch: epoch } = props;
  const target = voiceMenuTarget.value;
  if (
    !target ||
    !epoch ||
    voiceMenu.value?.downloadComplete !== true ||
    !voiceMenuLive(item, epoch, target)
  )
    return;
  try {
    await core.searchOpenVoiceDirectory(epoch, target);
  } catch (error) {
    if (voiceMenuLive(item, epoch, target)) emit('voiceMenuError', failure(error).message);
  }
}
watch(
  [() => props.item, () => props.voiceMenuEpoch],
  () => {
    voiceMenu.value = undefined;
    if (props.voiceMenuEpoch || voiceLinkMenuItem) menu.value?.close();
  },
  { flush: 'sync' },
);
let voiceLinkMenuItem: SearchItem | undefined;
let podcastMenuItem: SearchItem | undefined;
function captureActionMenu() {
  voiceLinkMenuItem = props.voiceLinks && props.item.kind === 'voice' ? props.item : undefined;
  podcastMenuItem = podcastRow.value && props.item.kind === 'podcast' ? props.item : undefined;
}
watch(
  [() => props.item, () => props.voiceLinks],
  () => {
    if (!voiceLinkMenuItem) return;
    voiceLinkMenuItem = undefined;
    menu.value?.close();
  },
  { flush: 'sync' },
);
watch(
  () => props.item,
  () => {
    if (!podcastMenuItem) return;
    podcastMenuItem = undefined;
    menu.value?.close(true);
  },
  { flush: 'sync' },
);
const followButton = ref<HTMLButtonElement>();
const followLabel = computed(() =>
  props.podcastFollow?.kind === 'reconcile'
    ? '核对收藏'
    : props.podcastFollow?.subscribed
      ? '取消收藏'
      : '收藏播客',
);
watch(
  () => !!props.podcastFollow,
  (available) => {
    if (available || document.activeElement !== followButton.value) return;
    void nextTick(() => {
      if (document.activeElement === document.body)
        element.value
          ?.querySelector<HTMLElement>('.search-card-open')
          ?.focus({ preventScroll: true });
    });
  },
);
const identity = searchCardKey(props.item);
const playbackFocus = usePlaybackTriggerFocus(
  () => props.playable,
  element,
  () =>
    searchCardKey(props.item) === identity &&
    !element.value?.closest('.page-leave-active,.search-results-leave-active'),
);
function contextMenu(event: MouseEvent | KeyboardEvent) {
  cancelRowLink();
  const focused =
    event instanceof KeyboardEvent && event.target instanceof Element
      ? event.target.closest<HTMLButtonElement>('button:not(:disabled)')
      : null;
  const target =
    focused ??
    element.value?.querySelector<HTMLButtonElement>('.search-card-open:not(:disabled)') ??
    element.value?.querySelector<HTMLButtonElement>(
      '.search-card-play:not(:disabled), .menu-trigger',
    );
  const rect = target?.getBoundingClientRect() ?? element.value?.getBoundingClientRect();
  if (!rect) return;
  const pointer = event instanceof MouseEvent && (event.clientX || event.clientY);
  void menu.value?.showAt(
    pointer ? event.clientX : rect.left + rect.width / 2,
    pointer ? event.clientY : rect.top + rect.height / 2,
    target ?? undefined,
  );
}
const failedCover = ref(false);
const podcastRow = computed(() => props.category === 'podcasts' && !!props.tableRow);
const canPlay = computed(() =>
  props.item.kind === 'podcast'
    ? podcastRow.value
      ? !!searchPodcastRowSelection(props.item, 'index')
      : !!searchPodcastCardSelection(props.item)
    : searchCardCanPlay(props.item) || (props.item.kind === 'playlist' && props.current),
);
const canQueue = computed(
  () => props.queueable && (props.item.kind !== 'playlist' || !props.item.knownEmpty),
);
const albumArtists = computed(() => searchAlbumCredits(props.item));
const albumAlias = computed(() =>
  props.tableRow ? searchAlbumAlias(props.item, props.matchKeyword) : '',
);
const albumPublished = computed(() =>
  props.item.kind === 'album' ? albumCardDate(props.item.album.publishedAt) : '',
);
const cardArtists = computed(() =>
  props.item.kind === 'mv' ? searchVideoCredits(props.item) : albumArtists.value,
);
const playlistCreator = computed(() => searchPlaylistCreator(props.item));
const playlistTrackCount = computed(() =>
  props.item.kind === 'playlist' && (props.item.knownEmpty || props.item.playlist.trackCount > 0)
    ? `${props.item.playlist.trackCount} 首`
    : '',
);
const playlistSubscribe = computed(() =>
  props.item.kind === 'playlist'
    ? playlistSubscribeMenuItem(props.item.playlist, props.playlistKnown, props.playlistActionBusy)
    : undefined,
);
const canPlayMv = computed(() => !!searchMvPlaybackTarget(props.item));
const videoStats = computed(() => searchVideoCoverStats(props.item));
const videoStatsId = useId();
const playlistCountId = useId();
const playlistCount = computed(() =>
  props.category === 'all' && props.item.kind === 'playlist'
    ? playlistCoverCount(props.item.playlist.playCount, props.item.playlist.private)
    : null,
);
const rowPlayCount = computed(() =>
  props.tableRow && props.item.kind === 'playlist'
    ? playlistTableCount(props.item.playlist.playCount, props.item.playlist.private)
    : null,
);
const podcastPlayCount = computed(() =>
  props.item.kind === 'podcast' ? playlistTableCount(props.item.table?.playCount, false) : null,
);
const markedTitle = computed(
  () =>
    ['playlist', 'podcast', 'voice', 'mv'].includes(props.item.kind) ||
    (props.item.kind === 'artist' && props.category === 'artists') ||
    (props.item.kind === 'user' && props.category === 'users'),
);
const items = computed<MenuItem[]>(() => {
  if (props.item.kind === 'artist') return artistCardMenu();
  if (podcastRow.value && props.item.kind === 'podcast')
    return [
      { key: 'open', label: '查看播客', icon: 'forward', disabled: unavailable.value },
      { key: 'play', label: '播放', icon: 'play', disabled: !props.playable || !canPlay.value },
      {
        key: 'next',
        label: '下一首播放',
        icon: 'next',
        disabled: !props.podcastNextEnabled || !canPlay.value,
      },
    ];
  return [
    ...(canPlay.value || props.item.kind === 'playlist'
      ? [
          {
            key: 'play',
            label:
              !podcastRow.value && !ordinaryVoiceMenu.value && props.current && props.playing
                ? '暂停'
                : '播放',
            icon:
              !podcastRow.value && !ordinaryVoiceMenu.value && props.current && props.playing
                ? 'pause'
                : 'play',
            disabled:
              !props.playable ||
              !canPlay.value ||
              (ordinaryVoiceMenu.value && (!voiceMenuTarget.value || props.voiceNext !== 'ready')),
          },
        ]
      : []),
    ...(props.item.kind === 'voice' && props.voiceNext
      ? [
          {
            key: 'next',
            label: '下一首播放',
            icon: 'next',
            disabled: props.voiceNext !== 'ready' || props.item.voice.unavailable,
          },
        ]
      : []),
    {
      key: 'open',
      label:
        props.item.kind === 'voice'
          ? '查看所属播客'
          : props.item.kind === 'user'
            ? '查看用户信息'
            : props.item.kind === 'mv'
              ? '播放 MV'
              : '查看详情',
      icon: props.item.kind === 'mv' ? 'play' : 'forward',
      disabled: unavailable.value,
    },
    ...(searchCardCanQueue(props.item)
      ? [{ key: 'next', label: '下一首播放', icon: 'next', disabled: !canQueue.value }]
      : []),
    ...(props.item.kind === 'playlist'
      ? [
          ...(playlistSubscribe.value ? [playlistSubscribe.value] : []),
          playlistLinkMenuItem(props.item.playlist, 'share'),
          playlistLinkMenuItem(props.item.playlist, 'copy'),
          {
            key: 'download-playlist',
            label: '下载全部',
            icon: 'download',
            disabled: props.playlistActionBusy || props.item.playlist.trackCount <= 0,
          },
        ]
      : []),
    ...(props.item.kind === 'album'
      ? [
          ...((props.albumSubscribed ?? props.item.album.subscribed) === true
            ? []
            : [{ key: 'collect-album', label: '收藏', icon: 'star' }]),
          {
            key: 'share',
            label: '分享…',
            icon: 'share',
            disabled: !albumLink(props.item.album.id),
          },
          {
            key: 'copy',
            label: '复制链接',
            icon: 'copy',
            disabled: !albumLink(props.item.album.id),
          },
        ]
      : []),
    ...(['playlist', 'album', 'voice', 'mv'].includes(props.item.kind)
      ? [
          {
            key: 'comments',
            label:
              props.item.kind === 'voice' && voiceMenu.value?.commentCount
                ? `查看评论(${voiceMenu.value.commentCount})`
                : '查看评论',
            icon: 'comment',
            disabled: props.item.kind === 'mv' && unavailable.value,
          },
        ]
      : []),
    ...(props.item.kind === 'voice' && voiceMenu.value?.downloadComplete !== true
      ? [
          {
            key: 'download',
            label: voiceMenuTarget.value ? '下载' : '下载声音',
            icon: 'download',
            disabled:
              props.item.voice.unavailable ||
              (!!voiceMenuTarget.value && voiceMenu.value?.downloadComplete == null),
          },
        ]
      : []),
    ...(props.item.kind === 'voice' && props.voiceLinks
      ? [
          {
            key: 'share',
            label: '分享…',
            icon: 'share',
            disabled: !podcastProgramPublicLink(props.item.voice.id),
          },
          {
            key: 'copy',
            label: '复制链接',
            icon: 'copy',
            disabled: !podcastProgramPublicLink(props.item.voice.id),
          },
        ]
      : []),
    ...(props.item.kind === 'voice' && voiceMenu.value?.downloadComplete === true
      ? [{ key: 'voice-folder', label: '打开文件所在目录', icon: 'folder' }]
      : []),
  ];
});
function actionAnchor() {
  const active = document.activeElement;
  return active instanceof HTMLElement && element.value?.contains(active)
    ? active
    : (element.value?.querySelector<HTMLElement>('.search-card-open') ?? null);
}
const playButtonPauses = computed(
  () => props.pendingVoiceToggle?.pause ?? (props.current && props.playing),
);
const playButtonDisabled = computed(() => props.pendingVoiceToggle?.disabled ?? !props.playable);
function playButton(event: MouseEvent) {
  const target = props.pendingVoiceToggle;
  if (target && event.currentTarget instanceof HTMLButtonElement) {
    if (!target.disabled) emit('togglePendingVoice', target, event.currentTarget);
    return;
  }
  action('play', 'index');
}
function selectMenuAction(key: string) {
  if (key !== 'play' || !ordinaryVoiceMenu.value) return action(key);
  const { item, voiceMenuEpoch: epoch } = props;
  const target = voiceMenuTarget.value;
  if (
    !target ||
    !epoch ||
    voiceLinkMenuItem !== item ||
    !voiceMenu.value ||
    !voiceMenuLive(item, epoch, target) ||
    !canPlay.value ||
    !props.playable ||
    props.voiceNext !== 'ready'
  )
    return;
  playbackFocus.start(
    () => emit('voiceMenuPlay', target),
    element.value?.querySelector<HTMLElement>('.menu-trigger'),
  );
}
function action(key: string, gesture: SearchPodcastRowGesture = 'menu') {
  cancelRowLink();
  if (key === 'voice-folder') void openVoiceFolder();
  else if (key === 'play' && canPlay.value && props.playable)
    playbackFocus.start(
      () =>
        emit(
          'play',
          props.item,
          podcastRow.value && props.item.kind === 'podcast' ? gesture : undefined,
        ),
      gesture === 'double_click'
        ? actionAnchor()
        : element.value?.querySelector<HTMLElement>('.search-card-play'),
    );
  else if (key === 'open' && !unavailable.value) emit('open', props.item);
  else if (props.item.kind === 'artist' && (key === 'collect' || key === 'copy')) {
    emit('artistAction', { artist: props.item.artist, action: key, anchor: actionAnchor() });
  } else if (key === 'next' && canQueue.value && searchCardCanQueue(props.item)) {
    emit('queue', { item: props.item, anchor: actionAnchor() });
  } else if (
    key === 'next' &&
    podcastRow.value &&
    props.item.kind === 'podcast' &&
    podcastMenuItem === props.item &&
    props.podcastNextEnabled &&
    canPlay.value &&
    !element.value?.closest('.page-leave-active,.search-results-leave-active')
  ) {
    emit('play', props.item, 'next');
  } else if (
    key === 'next' &&
    props.item.kind === 'voice' &&
    props.voiceNext === 'ready' &&
    !props.item.voice.unavailable
  ) {
    emit('queueVoice', props.item.voice.id);
  } else if (
    key === 'collect-album' &&
    props.item.kind === 'album' &&
    (props.albumSubscribed ?? props.item.album.subscribed) !== true
  ) {
    emit('albumAction', { album: props.item.album, action: 'collect', anchor: actionAnchor() });
  } else if (
    (key === 'share' || key === 'copy') &&
    props.item.kind === 'album' &&
    albumLink(props.item.album.id)
  ) {
    emit('albumAction', { album: props.item.album, action: key, anchor: actionAnchor() });
  } else if (
    key === 'subscribe' &&
    props.item.kind === 'playlist' &&
    playlistSubscribe.value?.disabled === false
  ) {
    emit('playlistSubscribe', props.item.playlist);
  } else if (
    (key === 'share' || key === 'copy') &&
    props.item.kind === 'playlist' &&
    !playlistLinkMenuItem(props.item.playlist, key).disabled
  ) {
    emit('playlistLink', props.item.playlist, key);
  } else if (
    (key === 'share' || key === 'copy') &&
    props.voiceLinks &&
    props.item.kind === 'voice' &&
    voiceLinkMenuItem === props.item
  ) {
    voiceLinkMenuItem = undefined;
    emit('voiceLink', { item: props.item, action: key, anchor: actionAnchor() });
  } else if (
    key === 'download-playlist' &&
    props.item.kind === 'playlist' &&
    !props.playlistActionBusy &&
    props.item.playlist.trackCount > 0
  ) {
    emit('playlistDownload', props.item.playlist);
  } else if (key === 'comments') emit('comments', props.item);
  else if (key === 'download' && props.item.kind === 'voice' && !props.item.voice.unavailable)
    emit('download', props.item);
}
// A Vue route change removes the row after the first physical click. Leave
// the podcast body a bounded native double-click interval, while explicit
// title/creator/index/menu controls remain immediate.
let rowLinkTimer: ReturnType<typeof setTimeout> | undefined;
function cancelRowLink() {
  if (rowLinkTimer === undefined) return;
  clearTimeout(rowLinkTimer);
  rowLinkTimer = undefined;
}
watch([() => props.item, () => props.category], cancelRowLink, { flush: 'sync' });
onBeforeUnmount(cancelRowLink);

function rowBody(event: MouseEvent) {
  return (
    !event.defaultPrevented &&
    event.target instanceof Element &&
    !event.target.closest('button, a, input, [role="menu"], .action-menu-host')
  );
}
function doubleClick(event: MouseEvent) {
  if (props.item.kind === 'voice') {
    if (!(event.target instanceof Element && event.target.closest('button, a, [role="menu"]')))
      action('play');
    return;
  }
  if (props.item.kind === 'podcast' && podcastRow.value && rowBody(event)) {
    cancelRowLink();
    action('play', 'double_click');
  }
}
function activateRow(event: MouseEvent) {
  if (
    !props.tableRow ||
    props.item.kind === 'voice' ||
    event.defaultPrevented ||
    !(event.target instanceof Element) ||
    event.target.closest('button, a, .action-menu-host')
  )
    return;
  if (props.item.kind !== 'podcast' || !podcastRow.value || event.detail === 0) {
    action('open');
    return;
  }
  if (!rowBody(event)) return;
  cancelRowLink();
  if (event.detail > 1) return;
  const item = props.item;
  rowLinkTimer = setTimeout(() => {
    rowLinkTimer = undefined;
    if (
      props.item === item &&
      element.value?.isConnected &&
      !element.value.closest('.page-leave-active,.search-results-leave-active')
    )
      action('open');
  }, 500);
}

const unavailable = computed(() =>
  props.item.kind === 'voice'
    ? !props.item.voice.radioId
    : props.item.kind === 'mv'
      ? !canPlayMv.value
      : props.item.kind === 'user' && props.item.user.unavailable,
);
const playlistRecommendation = computed(() =>
  props.item.kind === 'playlist' ? props.item.recommendation : undefined,
);
const compactVoiceCount = new Intl.NumberFormat('zh-CN', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
const voiceListens = computed(() => {
  const count = props.item.kind === 'voice' ? props.item.listenerCount : undefined;
  return count !== undefined && Number.isSafeInteger(count) && count >= 0
    ? compactVoiceCount.format(count)
    : undefined;
});
const voiceDuration = computed(() => {
  const ms = props.item.kind === 'voice' ? props.item.voice.durationMs : 0;
  return Number.isFinite(ms) && ms > 0 ? time(ms / 1000) : '';
});
const card = computed(() => {
  const i = props.item;
  switch (i.kind) {
    case 'artist':
      return {
        name: i.artist.name,
        cover: i.artist.coverUrl,
        sub:
          props.category === 'artists'
            ? `专辑：${i.artist.albumCount}`
            : i.artist.aliases.join(' / ') || `${i.artist.songCount} 首歌曲`,
        round: true,
      };
    case 'user':
      return {
        name: i.user.name,
        cover: i.user.avatarUrl,
        sub: i.user.signature || '暂无简介',
        round: true,
      };
    case 'playlist':
      return {
        name: i.playlist.name,
        cover: i.playlist.coverUrl,
        sub: [playlistTrackCount.value, i.playlist.creator.trim()].filter(Boolean).join(' · '),
        round: false,
      };
    case 'album':
      return {
        name: i.album.name,
        cover: i.album.coverUrl,
        sub: albumArtists.value.map((artist) => artist.name).join(' / '),
        round: false,
      };
    case 'podcast':
      return {
        name: i.podcast.name,
        cover: i.podcast.coverUrl,
        sub: `${i.podcast.programCount} 期 · ${i.podcast.creator}`,
        round: false,
      };
    case 'voice':
      return {
        name: i.voice.name,
        cover: i.voice.coverUrl,
        sub: i.voice.radioName || i.voice.creator,
        round: false,
      };
    case 'mv':
      return {
        name: i.mv.title,
        cover: i.mv.coverUrl,
        sub: i.mv.artists.map((a) => a.name).join(' / '),
        round: false,
      };
    case 'related':
      return { name: i.keyword, cover: null, sub: '', round: false };
    case 'song':
      return {
        name: i.track.name,
        cover: i.track.coverUrl,
        sub: i.track.artists.join(' / '),
        round: false,
      };
  }
});
watch(
  () => card.value.cover,
  () => {
    failedCover.value = false;
  },
);
</script>
<template>
  <component
    :is="tableRow ? 'div' : 'article'"
    ref="element"
    class="search-result-card"
    :role="tableRow ? 'row' : undefined"
    :aria-rowindex="tableRow ? tableRow.index + 2 : undefined"
    :data-result-key="searchCardKey(item)"
    :class="{
      'search-person-card': card.round,
      'search-related-word': item.kind === 'related',
      'search-video-card': item.kind === 'mv',
      'search-cover-card': !tableRow && ['playlist', 'album', 'podcast', 'mv'].includes(item.kind),
      'search-collection-row': tableRow,
      'search-playlist-row': tableRow && item.kind === 'playlist',
      'search-album-row': tableRow && item.kind === 'album',
      'search-voice-row': tableRow && item.kind === 'voice',
      'search-podcast-row': tableRow && item.kind === 'podcast',
      'search-collection-striped': tableRow && tableRow.index % 2 === 0,
      'search-voice-card': !tableRow && item.kind === 'voice',
      'search-card-current': current,
      'search-card-unavailable': unavailable && !canPlay,
    }"
    @contextmenu.prevent="contextMenu"
    @keydown.shift.f10.prevent="contextMenu"
    @click="activateRow"
    @dblclick="doubleClick"
  >
    <template v-if="tableRow && ['playlist', 'album', 'voice', 'podcast'].includes(item.kind)">
      <div class="search-collection-number" :class="`search-${item.kind}-number`" role="cell">
        <span>{{ String(tableRow.number).padStart(2, '0') }}</span>
        <button
          v-if="
            (item.kind === 'album' || item.kind === 'voice' || item.kind === 'podcast') && canPlay
          "
          type="button"
          class="search-card-play"
          data-result-action="play"
          :disabled="playButtonDisabled"
          :aria-label="`${playButtonPauses ? '暂停' : '播放'} ${card.name}`"
          @click.stop="playButton"
          @dblclick.stop
        >
          <Icon :name="playButtonPauses ? 'pause' : 'play'" />
        </button>
      </div>
      <div
        class="search-collection-title-cell"
        :class="{
          'search-voice-title-cell': item.kind === 'voice',
          'search-podcast-follow-cell': item.kind === 'podcast' && podcastFollow,
        }"
        role="cell"
      >
        <span class="search-result-art">
          <img
            v-if="card.cover && !failedCover"
            :src="artworkAt(card.cover, 96)"
            alt=""
            loading="lazy"
            decoding="async"
            referrerpolicy="no-referrer"
            @error="failedCover = true"
          />
          <Icon v-else name="music" />
          <span
            v-if="item.kind === 'playlist' && canPlay"
            class="search-card-mask"
            aria-hidden="true"
          />
          <button
            v-if="item.kind === 'playlist' && canPlay"
            type="button"
            class="search-card-play"
            data-result-action="play"
            :disabled="!playable"
            :aria-label="`${current && playing ? '暂停' : '播放'} ${card.name}`"
            @click.stop="action('play')"
          >
            <Icon :name="current && playing ? 'pause' : 'play'" />
          </button>
        </span>
        <button
          v-if="item.kind !== 'voice'"
          type="button"
          class="search-card-open"
          :class="{
            'search-podcast-title': item.kind === 'podcast',
            'search-playlist-table-title': item.kind === 'playlist',
          }"
          data-result-action="open"
          :aria-label="`查看 ${card.name}`"
          :aria-description="playlistRecommendation"
          :data-tooltip="card.name"
          @click.stop="action('open')"
        >
          <span v-if="item.kind === 'album'" class="search-album-name">
            <span class="search-album-title-copy">
              <SearchMatchText :text="card.name" :keyword="matchKeyword" />
            </span>
            <span
              v-if="item.album.explicit"
              class="search-track-explicit search-album-explicit"
              role="img"
              aria-label="含成人内容"
              >E</span
            >
            <span v-if="albumAlias" class="search-album-table-alias">
              （<SearchMatchText :text="albumAlias" :keyword="matchKeyword" />）
            </span>
          </span>
          <template v-else-if="item.kind === 'playlist'">
            <span class="search-playlist-table-name">
              <SearchMatchText :text="card.name" :keyword="matchKeyword" />
            </span>
            <span
              v-if="playlistRecommendation"
              class="search-playlist-table-recommendation"
              :data-tooltip="playlistRecommendation"
            >
              <SearchMatchText :text="playlistRecommendation" :keyword="matchKeyword" />
            </span>
          </template>
          <template v-else-if="item.kind === 'podcast'">
            <span class="search-result-name" :data-tooltip="card.name">
              <SearchMatchText :text="card.name" :keyword="matchKeyword" />
            </span>
            <span
              v-if="item.table?.recommendation"
              class="search-podcast-recommendation"
              :data-tooltip="item.table.recommendation"
            >
              {{ item.table.recommendation }}
            </span>
          </template>
          <SearchMatchText v-else :text="card.name" :keyword="matchKeyword" />
        </button>
        <div v-else class="search-voice-title-copy">
          <span class="search-result-name" :data-tooltip="card.name">
            <SearchMatchText :text="card.name" :keyword="matchKeyword" />
          </span>
          <span
            v-if="item.voice.creator || item.voice.radioId || item.voice.unavailable"
            class="search-voice-attribution"
          >
            <button
              v-if="item.creatorId && item.voice.creator"
              type="button"
              class="search-voice-author search-voice-creator"
              data-result-action="voice-creator"
              :aria-label="`查看主播 ${item.voice.creator} 的主页`"
              :data-tooltip="item.voice.creator"
              @click.stop="emit('creator', item.creatorId)"
              @dblclick.stop
            >
              by&nbsp;<SearchMatchText :text="item.voice.creator" :keyword="matchKeyword" />
            </button>
            <span
              v-else-if="item.voice.creator"
              class="search-voice-author"
              :data-tooltip="item.voice.creator"
            >
              by&nbsp;<SearchMatchText :text="item.voice.creator" :keyword="matchKeyword" />
            </span>
            <button
              v-if="item.voice.radioId"
              type="button"
              class="search-voice-radio"
              data-result-action="voice-radio"
              :data-tooltip="item.voice.radioName || '查看所属播客'"
              @click.stop="action('open')"
              @dblclick.stop
            >
              {{ item.voice.radioName ? `播单：${item.voice.radioName}` : '查看所属播客' }}
            </button>
            <span v-else-if="item.voice.unavailable" class="search-voice-author"
              >单集暂不可播放</span
            >
          </span>
        </div>
        <div v-if="item.kind === 'voice'" class="search-voice-title-actions">
          <button
            type="button"
            class="icon-button"
            data-result-action="voice-download"
            :aria-label="`下载 ${card.name}`"
            :disabled="item.voice.unavailable"
            @click.stop="action('download')"
            @dblclick.stop
          >
            <Icon name="download" />
          </button>
          <button
            type="button"
            class="icon-button"
            data-result-action="voice-comments"
            :aria-label="`查看评论：${card.name}`"
            @click.stop="action('comments')"
            @dblclick.stop
          >
            <Icon name="comment" />
          </button>
          <ActionMenu
            ref="menu"
            class="search-card-menu"
            :label="`更多操作：${card.name}`"
            :items="items"
            :before-open="voiceMenuTarget ? prepareVoiceMenu : undefined"
            @opened="captureActionMenu"
            @select="selectMenuAction"
          />
        </div>
        <div
          v-else-if="item.kind === 'podcast' && podcastFollow"
          class="search-podcast-title-actions"
        >
          <button
            ref="followButton"
            type="button"
            class="icon-button search-podcast-follow"
            data-result-action="podcast-follow"
            :aria-label="`${followLabel}：${card.name}`"
            :data-tooltip="followLabel"
            :aria-disabled="podcastFollow.busy"
            :aria-pressed="podcastFollow.kind === 'toggle' ? podcastFollow.subscribed : undefined"
            @click.stop="!podcastFollow.busy && emit('podcastFollow', podcastFollow)"
            @dblclick.stop
          >
            <Icon :name="podcastFollow.kind === 'reconcile' ? 'refresh' : 'star'" />
          </button>
          <ActionMenu
            ref="menu"
            class="search-card-menu"
            :label="`更多操作：${card.name}`"
            :items="items"
            @opened="captureActionMenu"
            @select="action"
          />
        </div>
        <ActionMenu
          v-else
          ref="menu"
          class="search-card-menu"
          :label="`更多操作：${card.name}`"
          :items="items"
          @opened="captureActionMenu"
          @select="action"
        />
      </div>
      <template v-if="item.kind === 'playlist'">
        <span class="search-playlist-track-count" role="cell">{{ playlistTrackCount || '—' }}</span>
        <div class="search-playlist-creator-cell" role="cell">
          <button
            v-if="playlistCreator"
            type="button"
            class="search-playlist-creator"
            data-result-action="creator"
            :aria-label="`查看创建者 ${playlistCreator.name} 的主页`"
            :data-tooltip="playlistCreator.name"
            @click.stop="emit('creator', playlistCreator.id)"
          >
            {{ playlistCreator.name }}
          </button>
          <span
            v-else
            class="search-playlist-creator-text"
            :data-tooltip="item.playlist.creator || undefined"
            >{{ item.playlist.creator || '—' }}</span
          >
        </div>
        <span class="search-playlist-play-count" role="cell" :data-tooltip="rowPlayCount?.label">
          <template v-if="rowPlayCount"
            ><span aria-hidden="true">{{ rowPlayCount.text }}</span
            ><span class="sr-only">{{ rowPlayCount.label }}</span></template
          >
          <template v-else>—</template>
        </span>
      </template>
      <template v-else-if="item.kind === 'album'">
        <div class="search-album-author-cell search-card-credit" role="cell">
          <SearchArtistCredits
            v-if="albumArtists.length"
            :artists="albumArtists"
            :keyword="matchKeyword"
            @artist="emit('artist', $event)"
          />
          <span v-else>—</span>
        </div>
        <div class="search-album-time-cell" role="cell">
          <span v-if="albumPublished" class="search-album-date">
            <time :datetime="albumPublished">{{ albumPublished }}</time>
          </span>
          <span v-else>—</span>
        </div>
      </template>
      <template v-else-if="item.kind === 'voice'">
        <span class="search-voice-listens search-voice-stat-cell" role="cell">
          {{ voiceListens ?? '—' }}
        </span>
        <span class="search-voice-duration search-voice-stat-cell" role="cell">
          {{ voiceDuration || '—' }}
        </span>
      </template>
      <template v-else-if="item.kind === 'podcast'">
        <div class="search-podcast-creator-cell" role="cell">
          <button
            v-if="item.table?.creatorId && item.podcast.creator"
            type="button"
            class="search-podcast-creator"
            data-result-action="creator"
            :aria-label="`查看主播 ${item.podcast.creator} 的主页`"
            :data-tooltip="item.podcast.creator"
            @click.stop="emit('creator', item.table.creatorId)"
            @dblclick.stop
          >
            <SearchMatchText :text="item.podcast.creator" :keyword="matchKeyword" />
          </button>
          <span v-else>
            <SearchMatchText :text="item.podcast.creator || '—'" :keyword="matchKeyword" />
          </span>
        </div>
        <span class="search-podcast-stat search-podcast-program-count" role="cell">
          {{ item.table?.programCount == null ? '—' : `${item.table.programCount} 个` }}
        </span>
        <span
          class="search-podcast-stat search-podcast-play-count"
          role="cell"
          :data-tooltip="podcastPlayCount?.label"
        >
          <template v-if="podcastPlayCount">
            <span aria-hidden="true">{{ podcastPlayCount.text }}</span>
            <span class="sr-only">{{ podcastPlayCount.label }}</span>
          </template>
          <template v-else>—</template>
        </span>
        <span class="search-podcast-stat search-podcast-score" role="cell">
          {{ item.table?.score ? `${item.table.score} 分` : '—' }}
        </span>
      </template>
    </template>
    <template v-else>
      <button
        v-if="item.kind !== 'voice'"
        type="button"
        class="search-card-open"
        :disabled="unavailable"
        :aria-label="`${item.kind === 'user' ? '查看用户主页' : item.kind === 'mv' ? '播放 MV' : '查看'} ${card.name}`"
        :aria-describedby="videoStats ? videoStatsId : playlistCount ? playlistCountId : undefined"
        :data-tooltip="[card.name, card.sub].filter(Boolean).join(' · ')"
        @click="action('open')"
      />
      <span v-if="item.kind !== 'related'" class="search-result-art">
        <img
          v-if="card.cover && !failedCover"
          :src="artworkAt(card.cover, 320)"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
          @error="failedCover = true"
        />
        <Icon v-else :name="card.round ? 'user' : 'music'" />
        <span v-if="canPlay || canPlayMv" class="search-card-mask" aria-hidden="true" />
        <span v-if="canPlayMv" class="search-video-play-glyph" aria-hidden="true">
          <Icon name="play" />
        </span>
        <template v-if="playlistCount">
          <span class="search-playlist-count" aria-hidden="true">
            <Icon name="play" />{{ playlistCount.text }}
          </span>
          <span :id="playlistCountId" class="sr-only">{{ playlistCount.label }}</span>
        </template>
        <template v-if="videoStats">
          <span v-if="videoStats.count" class="search-video-count" aria-hidden="true">
            {{ videoStats.count }}
          </span>
          <span v-if="videoStats.duration" class="search-video-duration" aria-hidden="true">
            {{ videoStats.duration }}
          </span>
          <span :id="videoStatsId" class="sr-only">{{ videoStats.description }}</span>
        </template>
        <button
          v-if="canPlay"
          type="button"
          class="search-card-play"
          :disabled="playButtonDisabled"
          :aria-label="`${playButtonPauses ? '暂停' : '播放'} ${card.name}`"
          @click.stop="playButton"
          @dblclick.stop
        >
          <Icon :name="playButtonPauses ? 'pause' : 'play'" />
        </button>
      </span>
      <span
        class="search-result-name"
        :class="{ 'search-album-name': item.kind === 'album' }"
        :data-tooltip="card.name"
      >
        <template v-if="item.kind === 'album'">
          <span class="search-album-title-copy">
            <SearchMatchText :text="card.name" :keyword="matchKeyword" />
          </span>
          <span
            v-if="item.album.explicit"
            class="search-track-explicit search-album-explicit"
            role="img"
            aria-label="含成人内容"
            >E</span
          >
        </template>
        <SearchMatchText v-else-if="markedTitle" :text="card.name" :keyword="matchKeyword" />
        <template v-else>{{ card.name }}</template>
      </span>
      <template v-if="item.kind === 'voice'">
        <span class="search-voice-footer">
          <span v-if="voiceDuration" class="search-voice-duration">{{ voiceDuration }}</span>
          <span v-if="voiceListens !== undefined" class="search-voice-listens"
            >播放：{{ voiceListens }}</span
          >
          <button
            v-if="item.voice.radioId"
            type="button"
            class="search-voice-radio"
            data-result-action="voice-radio"
            :data-tooltip="item.voice.radioName || '查看所属播客'"
            @click.stop="emit('open', item)"
            @dblclick.stop
          >
            {{ item.voice.radioName ? `播单：${item.voice.radioName}` : '查看所属播客' }}
          </button>
          <span v-else-if="item.voice.unavailable" class="search-result-sub">单集暂不可播放</span>
          <span
            v-else-if="item.voice.creator"
            class="search-result-sub"
            :data-tooltip="item.voice.creator"
          >
            {{ item.voice.creator }}
          </span>
        </span>
        <button
          type="button"
          class="icon-button search-card-download"
          :aria-label="`下载 ${card.name}`"
          :disabled="item.voice.unavailable"
          @click.stop="action('download')"
          @dblclick.stop
        >
          <Icon name="download" />
        </button>
      </template>
      <span
        v-else-if="cardArtists.length || albumPublished"
        class="search-result-sub search-card-credit"
      >
        <SearchArtistCredits
          :artists="cardArtists"
          :keyword="matchKeyword"
          @artist="emit('artist', $event)"
        />
        <span v-if="albumPublished" class="search-album-date"
          ><span v-if="cardArtists.length" aria-hidden="true">· </span
          ><time :datetime="albumPublished">{{ albumPublished }}</time></span
        >
      </span>
      <span
        v-else-if="item.kind === 'playlist' && (playlistCreator || playlistRecommendation)"
        class="search-result-sub search-playlist-credit"
      >
        <span v-if="!playlistRecommendation && playlistTrackCount"
          >{{ playlistTrackCount }}{{ playlistCreator || item.playlist.creator ? ' ·' : '' }}</span
        >
        <button
          v-if="playlistCreator"
          type="button"
          class="search-playlist-creator"
          data-result-action="creator"
          :aria-label="`查看创建者 ${playlistCreator.name} 的主页`"
          :data-tooltip="playlistCreator.name"
          @click.stop="emit('creator', playlistCreator.id)"
          @dblclick.stop
        >
          {{ playlistCreator.name }}
        </button>
        <span v-else-if="item.playlist.creator" class="search-playlist-creator-text">{{
          item.playlist.creator
        }}</span>
        <span v-if="playlistRecommendation" class="search-playlist-recommendation">
          <span v-if="playlistCreator || item.playlist.creator" aria-hidden="true">· </span>
          <SearchMatchText :text="playlistRecommendation" :keyword="matchKeyword" />
        </span>
      </span>
      <span v-else-if="unavailable && !canPlay" class="search-result-sub">{{
        item.kind === 'user' ? '此账号当前不可用' : '暂不可打开'
      }}</span>
      <span v-else-if="card.sub" class="search-result-sub" :data-tooltip="card.sub">{{
        card.sub
      }}</span>
      <ActionMenu
        v-if="item.kind !== 'related'"
        ref="menu"
        :context-only="item.kind !== 'voice'"
        class="search-card-menu"
        :label="`更多操作：${card.name}`"
        :items="items"
        :before-open="voiceMenuTarget ? prepareVoiceMenu : undefined"
        @opened="captureActionMenu"
        @select="selectMenuAction"
      />
    </template>
  </component>
</template>
