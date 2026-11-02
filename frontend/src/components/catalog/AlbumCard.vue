<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { AlbumCard, CatalogResource } from '../../services/catalog';
import {
  albumCredits,
  albumCardDate,
  type AlbumCardRequest,
  type AlbumCardAction,
} from '../../services/catalogCards';
import type { MenuItem } from '../../services/menu';
import { usePlaybackTriggerFocus } from '../../composables/usePlaybackTriggerFocus';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  album: AlbumCard;
  playable: boolean;
  queueable: boolean;
  current: boolean;
  playing: boolean;
  saved: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [album: AlbumCard];
  action: [request: AlbumCardRequest];
}>();
const element = ref<HTMLElement>(),
  menu = ref<InstanceType<typeof ActionMenu>>();
const identity = props.album.id;
const focus = usePlaybackTriggerFocus(
  () => props.playable,
  element,
  () =>
    props.album.id === identity &&
    !element.value?.closest('.page-leave-active,.catalog-tab-leave-active'),
);
const credits = computed(() => albumCredits(props.album));
const published = computed(() => albumCardDate(props.album.publishedAt));
const failedCover = ref(false);
watch(
  () => props.album.coverUrl,
  () => {
    failedCover.value = false;
  },
);
const items = computed<MenuItem[]>(() => [
  { key: 'open', label: '查看专辑', icon: 'forward' },
  {
    key: 'play',
    label: props.current && props.playing ? '暂停' : '播放',
    icon: props.current && props.playing ? 'pause' : 'play',
    disabled: !props.playable,
  },
  { key: 'next', label: '下一首播放', icon: 'next', disabled: !props.queueable },
  { key: 'separator', label: '', separator: true },
  ...(!props.saved && props.album.subscribed !== true
    ? [{ key: 'collect', label: '收藏专辑…', icon: 'star' }]
    : []),
  { key: 'share', label: '分享专辑…', icon: 'share' },
  { key: 'download', label: '下载专辑…', icon: 'download' },
  { key: 'copy', label: '复制链接', icon: 'copy' },
  ...(props.saved
    ? [
        { key: 'remove-separator', label: '', separator: true },
        { key: 'remove', label: '删除专辑…', icon: 'trash' },
      ]
    : []),
]);
function play() {
  focus.start(() => emit('play', props.album));
}
function action(key: string) {
  if (key === 'open') emit('open', { kind: 'album', id: props.album.id });
  else if (key === 'play') play();
  else if (items.value.some((item) => item.key === key && !item.disabled && !item.separator))
    emit('action', {
      album: props.album,
      action: key as AlbumCardAction,
      anchor: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    });
}
function context(event: MouseEvent | KeyboardEvent) {
  const target =
    event.target instanceof Element ? event.target.closest<HTMLButtonElement>('button') : null;
  const anchor = target ?? element.value?.querySelector<HTMLButtonElement>('.catalog-card');
  if (!anchor) return;
  const box = anchor.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.button === 2;
  void menu.value?.showAt(
    pointer ? event.clientX : box.left,
    pointer ? event.clientY : box.bottom,
    anchor,
  );
}
function keyboard(event: KeyboardEvent) {
  if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
    event.preventDefault();
    context(event);
  }
}
</script>
<template>
  <article
    ref="element"
    class="catalog-album-card"
    :class="{ 'catalog-album-current': current }"
    @contextmenu.prevent="context"
    @keydown="keyboard"
  >
    <button
      class="catalog-card catalog-album-open"
      :data-catalog-key="`album:${album.id}`"
      :aria-label="`查看专辑：${album.name}`"
      :data-tooltip="album.name"
      @click="emit('open', { kind: 'album', id: album.id })"
    />
    <span class="catalog-album-art">
      <img
        v-if="album.coverUrl && !failedCover"
        :src="album.coverUrl"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer"
        @error="failedCover = true"
      />
      <span v-else class="catalog-card-placeholder"><Icon name="music" /></span>
      <button
        class="catalog-album-play"
        :data-catalog-key="`album:${album.id}:play`"
        :aria-label="`${current && playing ? '暂停' : '播放'}专辑：${album.name}`"
        :disabled="!playable"
        @click="play"
      >
        <Icon :name="current && playing ? 'pause' : 'play'" />
      </button>
    </span>
    <div class="catalog-album-copy">
      <strong :title="album.name"
        ><span class="catalog-album-name">{{ album.name }}</span
        ><span v-if="album.explicit" class="album-explicit" aria-label="含成人内容">E</span></strong
      >
      <div class="catalog-album-description">
        <span
          v-if="credits.length"
          class="catalog-album-credits"
          :title="credits.map((artist) => artist.name).join(' / ')"
        >
          <template v-for="(artist, index) in credits" :key="index"
            ><span v-if="index"> / </span
            ><button
              v-if="artist.id"
              class="text-button"
              :data-catalog-key="`album:${album.id}:artist:${index}`"
              @click="emit('open', { kind: 'artist', id: artist.id })"
            >
              {{ artist.name }}</button
            ><span v-else>{{ artist.name }}</span></template
          >
        </span>
        <span v-if="published" class="catalog-album-date"
          >{{ credits.length ? '· ' : '' }}{{ published }}</span
        >
      </div>
    </div>
    <ActionMenu
      ref="menu"
      class="catalog-album-menu"
      variant="song"
      :label="`更多专辑操作：${album.name}`"
      :catalog-key="`album:${album.id}:menu`"
      :items="items"
      @select="action"
    />
  </article>
</template>
