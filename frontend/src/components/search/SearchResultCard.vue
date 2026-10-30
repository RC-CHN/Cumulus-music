<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { SearchItem } from '../../services/search';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
import type { MenuItem } from '../../services/menu';
import { searchCardCanPlay } from '../../services/searchCards';
const props = defineProps<{
  item: SearchItem;
  playable: boolean;
  current: boolean;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [item: SearchItem];
  play: [item: SearchItem];
  comments: [item: SearchItem];
  download: [item: SearchItem];
}>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const element = ref<HTMLElement>();
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
          : '查看详情',
    icon: 'forward',
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
  if (key === 'play' && canPlay.value && props.playable) emit('play', props.item);
  else if (key === 'open' && !unavailable.value) emit('open', props.item);
  else if (key === 'comments') emit('comments', props.item);
  else if (key === 'download') emit('download', props.item);
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
      ? !props.item.mv.mvId
      : false,
);
const card = computed(() => {
  const i = props.item;
  switch (i.kind) {
    case 'artist':
      return {
        name: i.artist.name,
        cover: i.artist.coverUrl,
        sub: i.artist.aliases.join(' / ') || `${i.artist.songCount} 首歌曲`,
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
        sub: `${i.playlist.trackCount} 首 · ${i.playlist.creator}`,
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
      :aria-label="`${item.kind === 'user' ? '查看用户信息' : '查看'} ${card.name}`"
      :data-tooltip="[card.name, card.sub].filter(Boolean).join(' · ')"
      @click="emit('open', item)"
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
      <span v-if="canPlay" class="search-card-mask" />
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
    <span class="search-result-name" :data-tooltip="card.name">{{ card.name }}</span>
    <button
      v-if="item.kind === 'voice' && item.voice.radioId"
      class="search-voice-radio"
      :data-tooltip="card.sub"
      @click.stop="emit('open', item)"
      @dblclick.stop
    >
      {{ card.sub }}
    </button>
    <span v-else-if="unavailable" class="search-result-sub">暂不可打开</span>
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
