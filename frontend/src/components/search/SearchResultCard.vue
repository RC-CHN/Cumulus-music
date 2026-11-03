<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue';
import type { SearchCategory, SearchItem } from '../../services/search';
import { artworkAt } from '../../services/artwork';
import { time } from '../../services/player';
import { playlistCoverCount } from '../../services/playlistHeader';
import Icon from '../Icon.vue';
import SearchMatchText from './SearchMatchText.vue';
import ActionMenu from '../ActionMenu.vue';
import type { MenuItem } from '../../services/menu';
import {
  searchAlbumCredits,
  searchCardCanPlay,
  searchCardCanQueue,
  searchCardKey,
  searchMvPlaybackTarget,
  searchPlaylistCreator,
  searchVideoCredits,
  searchVideoCoverStats,
  type SearchQueueRequest,
} from '../../services/searchCards';
import { usePlaybackTriggerFocus } from '../../composables/usePlaybackTriggerFocus';
const props = defineProps<{
  item: SearchItem;
  category: SearchCategory;
  matchKeyword: string;
  playable: boolean;
  queueable: boolean;
  current: boolean;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [item: SearchItem];
  creator: [id: string];
  artist: [id: string];
  play: [item: SearchItem];
  queue: [request: SearchQueueRequest];
  comments: [item: SearchItem];
  download: [item: SearchItem];
}>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const element = ref<HTMLElement>();
const identity = searchCardKey(props.item);
const playbackFocus = usePlaybackTriggerFocus(
  () => props.playable,
  element,
  () =>
    searchCardKey(props.item) === identity &&
    !element.value?.closest('.page-leave-active,.search-results-leave-active'),
);
function contextMenu(event: MouseEvent | KeyboardEvent) {
  const focused =
    event instanceof KeyboardEvent && event.target instanceof Element
      ? event.target.closest<HTMLButtonElement>('button:not(:disabled)')
      : null;
  const target =
    focused ??
    element.value?.querySelector<HTMLButtonElement>(
      '.search-card-open:not(:disabled), .search-card-play:not(:disabled), .menu-trigger',
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
const canPlay = computed(() => searchCardCanPlay(props.item));
const canQueue = computed(
  () => props.queueable && (props.item.kind !== 'playlist' || props.item.playlist.trackCount > 0),
);
const albumArtists = computed(() => searchAlbumCredits(props.item));
const cardArtists = computed(() =>
  props.item.kind === 'mv' ? searchVideoCredits(props.item) : albumArtists.value,
);
const playlistCreator = computed(() => searchPlaylistCreator(props.item));
const canPlayMv = computed(() => !!searchMvPlaybackTarget(props.item));
const videoStats = computed(() => searchVideoCoverStats(props.item));
const videoStatsId = useId();
const playlistCountId = useId();
const playlistCount = computed(() =>
  props.category === 'all' && props.item.kind === 'playlist'
    ? playlistCoverCount(props.item.playlist.playCount, props.item.playlist.private)
    : null,
);
const markedTitle = computed(
  () =>
    ['playlist', 'podcast', 'voice', 'mv'].includes(props.item.kind) ||
    (props.item.kind === 'artist' && props.category === 'artists') ||
    (props.item.kind === 'user' && props.category === 'users'),
);
const items = computed<MenuItem[]>(() => [
  ...(canPlay.value
    ? [
        {
          key: 'play',
          label: props.current && props.playing ? '暂停' : '播放',
          icon: props.current && props.playing ? 'pause' : 'play',
          disabled: !props.playable,
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
  ...(['playlist', 'album', 'voice', 'mv'].includes(props.item.kind)
    ? [
        {
          key: 'comments',
          label: '查看评论',
          icon: 'comment',
          disabled: props.item.kind === 'mv' && unavailable.value,
        },
      ]
    : []),
  ...(props.item.kind === 'voice'
    ? [
        {
          key: 'download',
          label: '下载声音',
          icon: 'download',
          disabled: props.item.voice.unavailable,
        },
      ]
    : []),
]);
function action(key: string) {
  if (key === 'play' && canPlay.value && props.playable)
    playbackFocus.start(
      () => emit('play', props.item),
      element.value?.querySelector<HTMLElement>('.search-card-play'),
    );
  else if (key === 'open' && !unavailable.value) emit('open', props.item);
  else if (key === 'next' && canQueue.value && searchCardCanQueue(props.item)) {
    const active = document.activeElement;
    const anchor =
      active instanceof HTMLElement && element.value?.contains(active)
        ? active
        : (element.value?.querySelector<HTMLElement>('.search-card-open') ?? null);
    emit('queue', { item: props.item, anchor });
  } else if (key === 'comments') emit('comments', props.item);
  else if (key === 'download' && props.item.kind === 'voice' && !props.item.voice.unavailable)
    emit('download', props.item);
}
function doubleClick(event: MouseEvent) {
  if (
    props.item.kind === 'voice' &&
    !(event.target instanceof Element && event.target.closest('button, a, [role="menu"]'))
  )
    action('play');
}

const unavailable = computed(() =>
  props.item.kind === 'voice'
    ? !props.item.voice.radioId
    : props.item.kind === 'mv'
      ? !canPlayMv.value
      : props.item.kind === 'user' && props.item.user.unavailable,
);
const playlistRecommendation = computed(() =>
  props.category === 'all' && props.item.kind === 'playlist'
    ? props.item.recommendation
    : undefined,
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
        sub: [`${i.playlist.trackCount} 首`, i.playlist.creator.trim()].filter(Boolean).join(' · '),
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
  <article
    ref="element"
    class="search-result-card"
    :data-result-key="searchCardKey(item)"
    :class="{
      'search-person-card': card.round,
      'search-related-word': item.kind === 'related',
      'search-video-card': item.kind === 'mv',
      'search-cover-card': ['playlist', 'album', 'podcast', 'mv'].includes(item.kind),
      'search-voice-card': item.kind === 'voice',
      'search-card-current': current,
      'search-card-unavailable': unavailable && !canPlay,
    }"
    @contextmenu.prevent="contextMenu"
    @keydown.shift.f10.prevent="contextMenu"
    @dblclick="doubleClick"
  >
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
        :disabled="!playable"
        :aria-label="`${current && playing ? '暂停' : '播放'} ${card.name}`"
        @click.stop="action('play')"
        @dblclick.stop
      >
        <Icon :name="current && playing ? 'pause' : 'play'" />
      </button>
    </span>
    <span class="search-result-name" :data-tooltip="card.name">
      <SearchMatchText v-if="markedTitle" :text="card.name" :keyword="matchKeyword" />
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
    <span v-else-if="cardArtists.length" class="search-result-sub search-card-credit">
      <template v-for="(artist, index) in cardArtists" :key="index">
        <span v-if="index" class="search-card-credit-separator" aria-hidden="true">/</span>
        <button
          v-if="artist.id"
          type="button"
          class="search-card-artist"
          :data-result-action="`artist:${artist.id}:${index}`"
          :aria-label="`查看歌手 ${artist.name}`"
          :data-tooltip="artist.name"
          @click.stop="emit('artist', artist.id)"
          @dblclick.stop
        >
          <SearchMatchText :text="artist.name" :keyword="matchKeyword" />
        </button>
        <span v-else class="search-card-artist-text">
          <SearchMatchText :text="artist.name" :keyword="matchKeyword" />
        </span>
      </template>
    </span>
    <span
      v-else-if="item.kind === 'playlist' && (playlistCreator || playlistRecommendation)"
      class="search-result-sub search-playlist-credit"
    >
      <span v-if="!playlistRecommendation">{{ item.playlist.trackCount }} 首 ·</span>
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
      @select="action"
    />
  </article>
</template>
