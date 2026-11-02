<script setup lang="ts">
import { ref, watch } from 'vue';
import type { ArtistCard, CatalogResource } from '../../services/catalog';
import { artistCardMenu, type ArtistCardRequest } from '../../services/artistCards';
import { usePlaybackTriggerFocus } from '../../composables/usePlaybackTriggerFocus';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
const props = defineProps<{
  artist: ArtistCard;
  playable: boolean;
  current: boolean;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [artist: ArtistCard];
  action: [request: ArtistCardRequest];
}>();
const element = ref<HTMLElement>(),
  menu = ref<InstanceType<typeof ActionMenu>>();
const identity = props.artist.id;
const focus = usePlaybackTriggerFocus(
  () => props.playable,
  element,
  () =>
    props.artist.id === identity &&
    !element.value?.closest('.page-leave-active,.catalog-tab-leave-active'),
);
const failed = ref(false);
watch(
  () => props.artist.coverUrl,
  () => {
    failed.value = false;
  },
);
const items = artistCardMenu();
function play() {
  focus.start(() => emit('play', props.artist));
}
function action(key: string) {
  if (key === 'open') emit('open', { kind: 'artist', id: props.artist.id });
  else if (key === 'collect' || key === 'copy')
    emit('action', {
      artist: props.artist,
      action: key,
      anchor: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    });
}
function context(event: MouseEvent | KeyboardEvent) {
  const anchor =
    (event.target instanceof Element ? event.target.closest<HTMLButtonElement>('button') : null) ??
    element.value?.querySelector<HTMLButtonElement>('.catalog-artist-open');
  if (!anchor) return;
  const box = anchor.getBoundingClientRect(),
    pointer = event instanceof MouseEvent && event.button === 2;
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
    class="catalog-artist-card"
    :class="{ 'catalog-artist-current': current }"
    @contextmenu.prevent="context"
    @keydown="keyboard"
  >
    <button
      class="catalog-card artist catalog-artist-open"
      :data-catalog-key="`artist:${artist.id}`"
      :aria-label="`查看歌手：${artist.name}`"
      :data-tooltip="artist.name"
      @click="emit('open', { kind: 'artist', id: artist.id })"
    />
    <span class="catalog-artist-art">
      <img
        v-if="artist.coverUrl && !failed"
        :src="artist.coverUrl"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer"
        @error="failed = true"
      />
      <span v-else class="catalog-card-placeholder"><Icon name="user" /></span>
      <span class="catalog-artist-mask" />
      <button
        class="catalog-artist-play"
        :data-catalog-key="`artist:${artist.id}:play`"
        :aria-label="`${current && playing ? '暂停' : '播放'}歌手：${artist.name}`"
        :disabled="!playable"
        @click="play"
      >
        <Icon :name="current && playing ? 'pause' : 'play'" />
      </button>
    </span>
    <strong class="catalog-artist-name" :title="artist.name">{{ artist.name }}</strong>
    <small class="catalog-artist-description">单曲：{{ artist.songCount }}</small>
    <ActionMenu
      ref="menu"
      class="catalog-artist-menu"
      variant="song"
      :label="`更多歌手操作：${artist.name}`"
      :catalog-key="`artist:${artist.id}:menu`"
      :items="items"
      @select="action"
    />
  </article>
</template>
