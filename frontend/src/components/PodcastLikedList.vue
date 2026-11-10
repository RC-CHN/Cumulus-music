<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue';
import { useOuterVirtualRows } from '../composables/useOuterVirtualRows';
import type { PodcastLikedModel } from '../composables/usePodcastLiked';
import { time } from '../services/player';
import type { PodcastLikedEpisode } from '../services/podcast';
import Icon from './Icon.vue';

const props = defineProps<{
  model: PodcastLikedModel;
  compact: boolean;
  playable: boolean;
  currentId?: string;
  currentActive: boolean;
  playing: boolean;
  removing: (id: string) => boolean;
}>();
const emit = defineEmits<{
  play: [voice: PodcastLikedEpisode, doubleClick: boolean, toggleCurrent?: boolean];
  menu: [voice: PodcastLikedEpisode, anchor: HTMLButtonElement, point?: { x: number; y: number }];
  comments: [voice: PodcastLikedEpisode];
  download: [voice: PodcastLikedEpisode];
  unlike: [voice: PodcastLikedEpisode];
}>();

const { page, pending, issue } = props.model;
const voices = computed(() => page.value?.episodes ?? []);
const rowHeight = computed(() => (props.compact ? 56 : 64));
const outer = (list?: HTMLElement) => list?.closest<HTMLElement>('.podcast-page') ?? undefined;
const { container, start, end, update } = useOuterVirtualRows(
  () => voices.value.length,
  () => rowHeight.value,
  outer,
  (element) =>
    Number(element.closest<HTMLElement>('[data-liked-voice-index]')?.dataset.likedVoiceIndex),
);
const visible = computed(() => voices.value.slice(start.value, end.value));
const formatCount = new Intl.NumberFormat('zh-CN', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
const notice = ref('');
let noticeTimer: ReturnType<typeof setTimeout> | undefined;

function withdrawn() {
  clearTimeout(noticeTimer);
  notice.value = '该声音已下线不可收听';
  noticeTimer = setTimeout(() => (notice.value = ''), 3000);
}
onUnmounted(() => clearTimeout(noticeTimer));

function play(voice: PodcastLikedEpisode, doubleClick: boolean, toggleCurrent = false) {
  if (voice.withdrawn) {
    withdrawn();
    return;
  }
  if (props.playable && !voice.unavailable) emit('play', voice, doubleClick, toggleCurrent);
}

function playIndex(voice: PodcastLikedEpisode) {
  const current = props.currentActive && props.currentId === voice.id;
  play(voice, !current, current);
}

function openMenu(event: MouseEvent | KeyboardEvent, voice: PodcastLikedEpisode) {
  if (voice.withdrawn || !(event.currentTarget instanceof HTMLElement)) return;
  const source = event.currentTarget;
  const anchor =
    source instanceof HTMLButtonElement
      ? source
      : source.querySelector<HTMLButtonElement>('[data-liked-voice-more]');
  if (!anchor) return;
  const point =
    event instanceof MouseEvent && event.button === 2
      ? { x: event.clientX, y: event.clientY }
      : undefined;
  emit('menu', voice, anchor, point);
}

function menuKey(event: KeyboardEvent, voice: PodcastLikedEpisode) {
  if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
  event.preventDefault();
  event.stopPropagation();
  openMenu(event, voice);
}

watch(
  [container, () => page.value?.revision],
  async ([list, revision]) => {
    const scroller = outer(list);
    if (!list || !scroller || !revision) return;
    const targetTop = props.model.scrollTop.value;
    await nextTick();
    if (container.value !== list || page.value?.revision !== revision) return;
    scroller.scrollTop = targetTop;
    update();
  },
  { flush: 'post' },
);
watch(rowHeight, (height, before) => {
  const list = container.value;
  const scroller = outer(list);
  if (!list || !scroller) return;
  const offset = scroller.getBoundingClientRect().top - list.getBoundingClientRect().top;
  const top = scroller.scrollTop;
  if (offset <= 0) return;
  void nextTick(() => {
    if (list !== container.value || rowHeight.value !== height || scroller.scrollTop !== top)
      return;
    scroller.scrollTop = top + (offset / before) * (height - before);
    props.model.scrollTop.value = scroller.scrollTop;
    update();
  });
});

watch(
  [() => page.value?.continuation, () => voices.value.length, start, end, pending, issue],
  () => {
    if (!page.value?.continuation || pending.value || issue.value) return;
    if (!voices.value.length || (end.value > start.value && end.value === voices.value.length))
      void props.model.next();
  },
  { flush: 'post' },
);
</script>

<template>
  <section
    class="liked-voice-table"
    :class="{ compact }"
    aria-label="点赞的单集"
    data-podcast-liked
  >
    <div v-if="voices.length" class="liked-voice-columns liked-voice-header" aria-hidden="true">
      <span>#</span><span>标题</span><span>播放量</span><span>时长</span>
    </div>
    <div
      v-if="voices.length"
      ref="container"
      class="liked-voice-rows"
      tabindex="-1"
      role="list"
      aria-label="点赞的单集列表"
      :style="{ height: voices.length * rowHeight + 'px' }"
    >
      <article
        v-for="(voice, offset) in visible"
        :key="voice.id"
        class="liked-voice-columns liked-voice-row"
        :class="{
          striped: (start + offset) % 2 === 0,
          current: currentId === voice.id,
          withdrawn: voice.withdrawn,
        }"
        :data-liked-voice-index="start + offset"
        :data-voice-id="voice.id"
        :style="{ top: (start + offset) * rowHeight + 'px', height: rowHeight + 'px' }"
        role="listitem"
        tabindex="0"
        :aria-haspopup="voice.withdrawn ? undefined : 'menu'"
        :aria-setsize="voices.length"
        :aria-posinset="start + offset + 1"
        @contextmenu.stop.prevent="openMenu($event, voice)"
        @keydown="menuKey($event, voice)"
        @keydown.enter.self="play(voice, true)"
        @click="voice.withdrawn && withdrawn()"
        @dblclick="play(voice, true)"
      >
        <button
          class="icon-button liked-voice-index"
          :aria-label="(currentId === voice.id && playing ? '暂停 ' : '播放 ') + voice.name"
          :disabled="!playable || voice.unavailable"
          @click.stop="playIndex(voice)"
          @dblclick.stop
        >
          <span aria-hidden="true">{{ start + offset + 1 }}</span>
          <Icon :name="currentId === voice.id && playing ? 'pause' : 'play'" />
        </button>
        <div class="liked-voice-title">
          <img v-if="voice.coverUrl" :src="voice.coverUrl" alt="" referrerpolicy="no-referrer" />
          <span v-else class="liked-voice-art" aria-hidden="true"><Icon name="headphones" /></span>
          <div class="liked-voice-info">
            <strong :title="voice.name">{{ voice.name }}</strong>
            <small>
              {{ voice.creator || voice.radioName || '单集' }}
              <span v-if="voice.withdrawn"> · 已下线</span>
              <span v-else-if="voice.feeType"> · 付费内容</span>
            </small>
          </div>
          <div v-if="!voice.withdrawn" class="liked-voice-actions" @dblclick.stop>
            <button
              class="icon-button"
              type="button"
              aria-label="下载单集"
              :disabled="voice.unavailable"
              @click.stop="emit('download', voice)"
            >
              <Icon name="down" />
            </button>
            <button
              class="icon-button"
              type="button"
              aria-label="单集评论"
              @click.stop="emit('comments', voice)"
            >
              <Icon name="comment" />
            </button>
            <button
              class="icon-button liked-voice-unlike"
              type="button"
              aria-label="取消单集点赞"
              :disabled="removing(voice.id)"
              @click.stop="emit('unlike', voice)"
            >
              <Icon name="heart" />
            </button>
            <button
              class="icon-button"
              type="button"
              data-liked-voice-more
              :aria-label="`更多：${voice.name}`"
              aria-haspopup="menu"
              aria-expanded="false"
              @click.stop="openMenu($event, voice)"
            >
              <Icon name="more" />
            </button>
          </div>
        </div>
        <span>{{
          voice.listenerCount == null ? '—' : formatCount.format(voice.listenerCount)
        }}</span>
        <span>{{ time(voice.durationMs / 1000) }}</span>
      </article>
    </div>
    <div v-if="notice" class="podcast-withdrawn-notice" role="status">{{ notice }}</div>
    <div v-if="issue" class="liked-voice-footer" role="alert">
      <span>{{ voices.length ? '无法继续读取，已保留当前列表。' : '无法读取点赞的单集。' }}</span>
      <button class="secondary" @click="model.retry">重试</button>
    </div>
    <div
      v-else-if="pending || (page && page.continuation && !voices.length)"
      class="liked-voice-footer"
      role="status"
    >
      <span class="spinner" />
      {{ voices.length ? '正在读取更多单集…' : '正在读取点赞的单集…' }}
    </div>
    <div v-else-if="page?.capped" class="liked-voice-footer" role="status">
      已达到本次读取上限。
    </div>
    <div v-else-if="page && !voices.length" class="empty-state">
      <Icon name="headphones" />
      <p>暂无点赞的单集</p>
    </div>
  </section>
</template>

<style scoped>
.liked-voice-table {
  --cover-size: 48px;
  --cover-radius: 8px;
  --title-size: 16px;
}
.liked-voice-table.compact {
  --cover-size: 36px;
  --cover-radius: 4px;
  --title-size: 14px;
}
.liked-voice-columns {
  display: grid;
  grid-template-columns: 55px minmax(0, 1fr) 92px 64px;
  align-items: center;
  column-gap: 12px;
  padding-inline: 12px;
  color: var(--muted);
}
.liked-voice-header {
  height: 32px;
  font-size: 12px;
}
.liked-voice-header > :first-child {
  text-align: center;
}
.liked-voice-rows {
  position: relative;
}
.liked-voice-row {
  position: absolute;
  inset-inline: 0;
  border-radius: 8px;
}
.liked-voice-row.striped {
  background: var(--surface-alt);
}
.liked-voice-row:is(:hover, :focus-within) {
  background: var(--surface-hover);
}
.liked-voice-index {
  justify-self: center;
}
.liked-voice-index .icon {
  display: none;
}
.liked-voice-row:is(:hover, :focus-within, .current) .liked-voice-index:not(:disabled) .icon {
  display: block;
}
.liked-voice-row:is(:hover, :focus-within, .current) .liked-voice-index:not(:disabled) span {
  display: none;
}
.liked-voice-title,
.liked-voice-actions {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
}
.liked-voice-title img,
.liked-voice-art {
  flex: 0 0 var(--cover-size);
  width: var(--cover-size);
  height: var(--cover-size);
  border-radius: var(--cover-radius);
  object-fit: cover;
}
.liked-voice-art {
  display: grid;
  place-items: center;
  background: var(--surface);
  color: var(--accent);
}
.liked-voice-info {
  flex: 1;
  min-width: 0;
}
.liked-voice-info > * {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.liked-voice-info strong {
  color: var(--text);
  font-size: var(--title-size);
}
.liked-voice-row.current strong,
.liked-voice-unlike {
  color: var(--accent);
}
.liked-voice-row.withdrawn strong {
  color: var(--muted);
}
.liked-voice-actions {
  flex: 0 0 auto;
  gap: 3px;
}
.liked-voice-actions .icon-button {
  width: 29px;
  height: 29px;
}
@media (hover: hover) {
  .liked-voice-row:not(:hover, :focus-within) .liked-voice-actions {
    opacity: 0;
    pointer-events: none;
  }
}
.liked-voice-footer {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 10px;
  padding: 16px 0;
  color: var(--muted);
}
@media (max-width: 640px) {
  .liked-voice-columns {
    grid-template-columns: 36px minmax(0, 1fr) 48px 42px;
    padding-inline: 5px;
    column-gap: 6px;
  }
  .liked-voice-actions {
    gap: 1px;
  }
  .liked-voice-actions .icon-button {
    width: 25px;
  }
}
</style>
