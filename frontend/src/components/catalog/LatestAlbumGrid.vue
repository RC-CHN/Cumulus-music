<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import AlbumItem from './AlbumCard.vue';
import type { AlbumCard, CatalogResource } from '../../services/catalog';
import type { AlbumCardRequest } from '../../services/catalogCards';
import type { LatestAlbumGroup } from '../../services/latestReleases';

const props = defineProps<{
  groups: LatestAlbumGroup[];
  playable: boolean;
  queueable: boolean;
  currentAlbumId?: string;
  playing: boolean;
  observedAlbumMembership: ReadonlyMap<string, boolean>;
  pending: boolean;
  more: boolean;
  autoMore: boolean;
  error: string;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [album: AlbumCard];
  action: [request: AlbumCardRequest];
  more: [];
  retry: [];
}>();
const scroller = ref<HTMLElement>();
const tail = ref<HTMLElement>();
const width = ref(800);
const height = ref(500);
const top = ref(0);
const gap = 16;
const columns = computed(() => Math.max(1, Math.min(4, Math.floor((width.value + gap) / 172))));
const cardRowHeight = computed(() =>
  Math.max(1, (width.value - gap * (columns.value - 1)) / columns.value + 72),
);
type Row = { key: string; group: string; top: number; height: number } & (
  | { heading: string }
  | { albums: AlbumCard[] }
);
const layout = computed(() => {
  const rows: Row[] = [];
  let offset = 0;
  for (const group of props.groups) {
    rows.push({ key: group.key, group: group.key, top: offset, height: 56, heading: group.title });
    offset += 56;
    for (let index = 0; index < group.albums.length; index += columns.value) {
      rows.push({
        key: `${group.key}:${index}`,
        group: group.key,
        top: offset,
        height: cardRowHeight.value,
        albums: group.albums.slice(index, index + columns.value),
      });
      offset += cardRowHeight.value;
    }
  }
  return { rows, height: offset };
});
function lowerBound(value: number) {
  const rows = layout.value.rows;
  let low = 0;
  let high = rows.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (rows[mid].top + rows[mid].height <= value) low = mid + 1;
    else high = mid;
  }
  return low;
}
const visible = computed(() =>
  layout.value.rows.slice(
    lowerBound(Math.max(0, top.value - cardRowHeight.value)),
    lowerBound(top.value + height.value + cardRowHeight.value) + 1,
  ),
);
let frame: number | undefined;
let timeout: ReturnType<typeof setTimeout> | undefined;
function clearScroll() {
  if (frame !== undefined) cancelAnimationFrame(frame);
  clearTimeout(timeout);
  frame = undefined;
}
function measureScroll() {
  clearScroll();
  top.value = scroller.value?.scrollTop ?? 0;
}
function onScroll() {
  if (frame !== undefined) return;
  frame = requestAnimationFrame(measureScroll);
  timeout = setTimeout(measureScroll, 64);
}
function restoreScroll(offset: number) {
  const node = scroller.value;
  if (node && Number.isFinite(offset)) node.scrollTop = Math.max(0, offset);
  measureScroll();
}
defineExpose({ restoreScroll });
onUnmounted(clearScroll);
watch(
  scroller,
  (node, _, cleanup) => {
    if (!node) return;
    const resize = () => {
      width.value = Math.max(1, node.clientWidth);
      height.value = node.clientHeight;
      measureScroll();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(node);
    cleanup(() => observer.disconnect());
    resize();
  },
  { flush: 'post' },
);
watch(
  [tail, scroller, () => props.pending, () => props.autoMore, () => props.error],
  ([node, root, pending, more, error], _, cleanup) => {
    if (!node || !root || pending || !more || error) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting && !props.pending && !props.error) emit('more');
      },
      { root, rootMargin: '0px 0px 36px 0px' },
    );
    observer.observe(node);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);

function displayAlbum(album: AlbumCard): AlbumCard {
  const observed = props.observedAlbumMembership.get(album.id);
  return observed === undefined || observed === album.subscribed
    ? album
    : { ...album, subscribed: observed };
}
</script>

<template>
  <div
    ref="scroller"
    class="latest-album-scroll"
    tabindex="0"
    aria-label="新碟列表"
    data-latest-scroll
    @scroll.passive="onScroll"
  >
    <div
      v-if="layout.rows.length"
      class="latest-album-layout"
      :style="{ height: `${layout.height}px` }"
      :data-latest-album-count="groups.reduce((count, group) => count + group.albums.length, 0)"
    >
      <div
        v-for="row in visible"
        :key="row.key"
        class="latest-album-row"
        :data-latest-group="row.group"
        :style="{
          transform: `translateY(${row.top}px)`,
          height: `${row.height}px`,
          gridTemplateColumns: 'albums' in row ? `repeat(${columns},minmax(0,1fr))` : undefined,
        }"
      >
        <h2 v-if="'heading' in row">{{ row.heading }}</h2>
        <AlbumItem
          v-for="album in 'albums' in row ? row.albums : []"
          :key="album.id"
          :album="displayAlbum(album)"
          :playable="playable"
          :queueable="queueable"
          :current="currentAlbumId === album.id"
          :playing="playing"
          :saved="false"
          @open="emit('open', $event)"
          @play="emit('play', $event)"
          @action="emit('action', $event)"
        />
      </div>
    </div>
    <div ref="tail" class="latest-album-tail">
      <p v-if="pending" class="muted" role="status">正在加载新碟…</p>
      <div v-else-if="error" class="latest-inline-error" role="alert">
        <p>{{ error }}</p>
        <button class="text-button" type="button" @click="emit('retry')">重试</button>
      </div>
      <button v-else-if="more" class="text-button" type="button" @click="emit('more')">
        加载更多
      </button>
      <p v-else-if="!groups.length" class="empty-state">暂无新碟</p>
    </div>
  </div>
</template>
