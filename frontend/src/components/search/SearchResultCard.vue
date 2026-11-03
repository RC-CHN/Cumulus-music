<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue';
import type { SearchCategory, SearchItem } from '../../services/search';
import { artworkAt } from '../../services/artwork';
import { time } from '../../services/player';
import Icon from '../Icon.vue';
import SearchMatchText from './SearchMatchText.vue';
import ActionMenu from '../ActionMenu.vue';
import type { MenuItem } from '../../services/menu';
import {
  searchCardCanPlay,
  searchCardKey,
  searchMvPlaybackTarget,
  searchPlaylistCreator,
  searchVideoCoverStats,
} from '../../services/searchCards';
import { usePlaybackTriggerFocus } from '../../composables/usePlaybackTriggerFocus';
const props = defineProps<{
  item: SearchItem;
  category: SearchCategory;
  matchKeyword: string;
  playable: boolean;
  current: boolean;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [item: SearchItem];
  creator: [id: string];
  play: [item: SearchItem];
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
const playlistCreator = computed(() => searchPlaylistCreator(props.item));
const canPlayMv = computed(() => !!searchMvPlaybackTarget(props.item));
const videoStats = computed(() => searchVideoCoverStats(props.item));
const videoStatsId = useId();
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
  else if (key === 'comments') emit('comments', props.item);
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
        sub: i.album.artists.map((a) => a.name).join(' / '),
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
      :aria-describedby="videoStats ? videoStatsId : undefined"
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
    <span
      v-else-if="item.kind === 'playlist' && playlistCreator"
      class="search-result-sub search-playlist-credit"
    >
      <span>{{ item.playlist.trackCount }} 首 ·</span>
      <button
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
