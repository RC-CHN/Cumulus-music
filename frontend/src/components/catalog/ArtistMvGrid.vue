<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { useVirtualRows } from '../../composables/useVirtualRows';
import type { ArtistVideoSlot } from '../../services/catalogPages';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';

const props = defineProps<{
  videos: ArtistVideoSlot[];
  continuation: string | null;
  loading: boolean;
  error: string;
}>();
const emit = defineEmits<{
  play: [id: string];
  comments: [id: string, title: string];
  more: [];
  retry: [];
}>();
const COLUMNS = 3;
const GAP = 16;
const width = ref(540);
const rowHeight = computed(
  () => Math.ceil(((Math.max(width.value, 200) - GAP * 2) * 0.565) / COLUMNS) + 38,
);
// Official waterfall holds the incomplete last row until its continuation.
const renderedCount = computed(() =>
  props.continuation ? props.videos.length - (props.videos.length % COLUMNS) : props.videos.length,
);
const rows = computed(() => Math.ceil(renderedCount.value / COLUMNS));
const {
  container: scroller,
  start,
  end,
  onScroll,
} = useVirtualRows(
  () => rows.value,
  () => rowHeight.value,
  2,
);
const visible = computed(() =>
  props.videos.slice(start.value * COLUMNS, Math.min(end.value * COLUMNS, renderedCount.value)),
);
const tail = ref<HTMLElement>();

watch(
  scroller,
  (element, _, cleanup) => {
    if (!element) return;
    const measure = new ResizeObserver(([entry]) => {
      width.value = entry?.contentRect.width ?? width.value;
    });
    measure.observe(element);
    cleanup(() => measure.disconnect());
  },
  { flush: 'post' },
);

let requested: string | null = null;
function more() {
  if (!props.continuation || props.loading || props.error || requested === props.continuation)
    return;
  requested = props.continuation;
  emit('more');
}
watch(
  () => props.error,
  (error) => {
    if (error) requested = null;
  },
);
watch(
  () => props.continuation,
  () => (requested = null),
);
watch(
  [
    scroller,
    tail,
    () => props.continuation,
    () => props.loading,
    () => props.error,
    () => props.videos.length,
  ],
  ([root, node, continuation, loading, error, count], _, cleanup) => {
    if (!root || !node || !continuation || loading || error || !count) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting && root.isConnected && !root.closest('[inert]')) more();
      },
      { root, rootMargin: '0px 0px 80px' },
    );
    observer.observe(node);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);

const fullCount = new Intl.NumberFormat('zh-CN');
const compactCount = new Intl.NumberFormat('zh-CN', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
function duration(ms: number) {
  const sec = Math.floor(ms / 1000);
  return `${String(Math.floor(sec / 60)).padStart(2, '0')}:${String(sec % 60).padStart(2, '0')}`;
}
function id(card: ArtistVideoSlot) {
  return card.video.kind === 'mv' ? card.video.mvId : null;
}
function play(card: ArtistVideoSlot) {
  const target = id(card);
  if (target) emit('play', target);
}
const menu = ref<InstanceType<typeof ActionMenu>>();
let selected: ArtistVideoSlot | undefined;
async function openMenu(event: MouseEvent | KeyboardEvent, card: ArtistVideoSlot) {
  if (!id(card) || !(event.currentTarget instanceof HTMLButtonElement)) return;
  if (
    event instanceof KeyboardEvent &&
    event.key !== 'ContextMenu' &&
    !(event.shiftKey && event.key === 'F10')
  )
    return;
  event.preventDefault();
  const button = event.currentTarget;
  const rect = button.getBoundingClientRect();
  const atPointer = event instanceof MouseEvent && (event.clientX !== 0 || event.clientY !== 0);
  selected = card;
  await nextTick();
  if (selected !== card || !button.isConnected || button.closest('[inert]')) return;
  void menu.value?.showAt(
    atPointer ? event.clientX : rect.left + 8,
    atPointer ? event.clientY : rect.bottom,
    button,
  );
}
function action(key: string) {
  const card = selected;
  selected = undefined;
  if (
    !card ||
    !props.videos.some(({ slot, video }) => slot === card.slot && video.mvId === card.video.mvId)
  )
    return;
  const target = id(card);
  if (!target) return;
  if (key === 'comments') emit('comments', target, card.video.title);
  else if (key === 'play') emit('play', target);
}
</script>

<template>
  <div
    v-if="videos.length || continuation"
    ref="scroller"
    class="catalog-card-scroll artist-mv-scroll"
    :data-catalog-total="videos.length"
    aria-label="歌手 MV"
    @scroll.passive="onScroll"
  >
    <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
    <div class="artist-mv-grid" :style="{ gridAutoRows: `${rowHeight - GAP}px` }">
      <article
        v-for="card in visible"
        :key="`${card.slot}:${card.video.id}`"
        class="catalog-video-card artist-mv-card"
        :data-artist-mv-slot="card.slot"
      >
        <button
          type="button"
          class="artist-mv-play"
          :disabled="!id(card)"
          :data-catalog-key="`artist-mv:${card.slot}:${card.video.id}`"
          :aria-label="id(card) ? `播放 MV ${card.video.title}` : `暂不可播放 ${card.video.title}`"
          :aria-haspopup="id(card) ? 'menu' : undefined"
          @click="play(card)"
          @contextmenu="openMenu($event, card)"
          @keydown="openMenu($event, card)"
        >
          <span class="artist-mv-cover">
            <img
              v-if="card.video.coverUrl"
              :src="card.video.coverUrl"
              alt=""
              loading="lazy"
              decoding="async"
              referrerpolicy="no-referrer"
            />
            <Icon v-else class="artist-mv-placeholder" name="video" />
            <span
              v-if="card.video.playCount > 0"
              class="artist-mv-count"
              :title="`播放次数：${fullCount.format(card.video.playCount)}`"
              ><Icon name="play" />{{ compactCount.format(card.video.playCount) }}</span
            >
            <span v-if="card.video.durationMs > 0" class="artist-mv-duration">{{
              duration(card.video.durationMs)
            }}</span>
            <span v-if="id(card)" class="artist-mv-overlay" aria-hidden="true"
              ><Icon name="play"
            /></span>
            <small v-else class="artist-mv-boundary">{{
              card.video.kind === 'video' ? '普通视频暂不可播' : '暂不支持此视频类型'
            }}</small>
          </span>
          <strong :title="card.video.title">{{ card.video.title }}</strong>
        </button>
      </article>
    </div>
    <div :style="{ height: `${(rows - end) * rowHeight}px` }" aria-hidden="true" />
    <div ref="tail" class="artist-mv-tail">
      <span v-if="error" role="alert"
        >{{ error }}{{ videos.length ? ' · 保留已加载内容' : '' }}
        <button type="button" class="text-button" @click="emit('retry')">重试</button></span
      >
      <button
        v-else-if="continuation"
        type="button"
        class="load-more"
        :disabled="loading"
        @click="more"
      >
        {{ loading ? '正在加载…' : '加载更多' }}
      </button>
    </div>
  </div>
  <div v-else-if="error" class="catalog-error" role="alert">
    {{ error }} <button type="button" class="text-button" @click="emit('retry')">重试</button>
  </div>
  <p v-else-if="loading" role="status" class="catalog-loading">正在加载…</p>
  <p v-else class="empty-state">暂无 MV</p>
  <ActionMenu
    ref="menu"
    context-only
    label="MV 操作"
    :items="[
      { key: 'comments', label: '查看评论', icon: 'comment' },
      { key: 'play', label: '播放', icon: 'play' },
    ]"
    @select="action"
  />
</template>

<style scoped>
.artist-mv-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 16px;
  margin-bottom: 16px;
}
.artist-mv-card .artist-mv-play {
  background: transparent;
  text-align: left;
  border-radius: 0;
  color: var(--text);
}
.artist-mv-cover {
  display: grid;
  position: relative;
  place-items: center;
  aspect-ratio: 100 / 56.5;
  background: var(--art-bg);
  overflow: hidden;
  border-radius: 8px;
}
.artist-mv-cover img {
  width: 100%;
  height: 100%;
  position: absolute;
  inset: 0;
  object-fit: cover;
}
.artist-mv-placeholder {
  width: 30px;
  height: 30px;
  color: var(--art-color);
}
.artist-mv-count,
.artist-mv-duration {
  position: absolute;
  padding: 2px 5px;
  border-radius: 4px;
  background: #0009;
  color: white;
  font-size: 11px;
}
.artist-mv-count {
  top: 5px;
  right: 5px;
  display: flex;
  align-items: center;
  gap: 3px;
}
.artist-mv-count svg {
  width: 12px;
  height: 12px;
}
.artist-mv-duration {
  bottom: 5px;
  right: 5px;
}
.artist-mv-overlay {
  display: grid;
  place-items: center;
  position: absolute;
  inset: 0;
  background: #0004;
  color: white;
  opacity: 0;
  transition: opacity 140ms ease;
}
.artist-mv-overlay svg {
  width: min(40%, 42px);
  height: min(48%, 42px);
}
.artist-mv-play:hover .artist-mv-overlay,
.artist-mv-play:focus-visible .artist-mv-overlay {
  opacity: 1;
}
.artist-mv-play strong {
  margin-top: 5px;
}
.artist-mv-play .artist-mv-boundary {
  position: absolute;
  bottom: 5px;
  left: 5px;
  padding: 2px 5px;
  border-radius: 4px;
  background: #0009;
  color: white;
  font-size: 10px;
}
.artist-mv-tail {
  text-align: center;
  min-height: 1px;
}
@media (prefers-reduced-motion: reduce) {
  .artist-mv-overlay {
    transition: none;
  }
}
</style>
