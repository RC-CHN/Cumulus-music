<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useOuterVirtualRows } from '../composables/useOuterVirtualRows';
import type { PodcastRecentModel } from '../composables/usePodcastRecent';
import type {
  EpisodeView,
  PodcastRecentEpisode,
  PodcastRecentProgramTarget,
} from '../services/podcast';
import PodcastProgramMenu from './PodcastProgramMenu.vue';
import { time } from '../services/player';
import Icon from './Icon.vue';

const props = defineProps<{
  model: PodcastRecentModel;
  compact: boolean;
  playable: boolean;
  queueable: boolean;
  queueProgram: (epoch: string, target: PodcastRecentProgramTarget) => Promise<boolean>;
  currentId?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  play: [revision: string, id: string, doubleClick: boolean];
  radio: [id: string, fromVoiceId: string];
  programPlay: [epoch: string, target: PodcastRecentProgramTarget];
  comments: [id: string, title: string];
  download: [voices: EpisodeView[]];
}>();
const { page, pending, issue } = props.model;
const episodes = computed(() => page.value?.episodes ?? []);
const height = computed(() => (props.compact ? 56 : 64));
const { container, start, end, update } = useOuterVirtualRows(
  () => episodes.value.length,
  () => height.value,
  (list) => list?.closest<HTMLElement>('.podcast-page') ?? undefined,
  (element) => Number(element.closest<HTMLElement>('[data-recent-index]')?.dataset.recentIndex),
);
watch(height, (after, before) => {
  const list = container.value;
  const scroller = list?.closest<HTMLElement>('.podcast-page');
  if (!list || !scroller) return;
  const scrolled = scroller.getBoundingClientRect().top - list.getBoundingClientRect().top;
  const top = scroller.scrollTop;
  if (scrolled <= 0) return;
  void nextTick(() => {
    if (container.value !== list || height.value !== after || scroller.scrollTop !== top) return;
    scroller.scrollTop = top + (scrolled / before) * (after - before);
    update();
  });
});
const visible = computed(() => episodes.value.slice(start.value, end.value));
const count = new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 });
function play(episode: PodcastRecentEpisode, doubleClick: boolean) {
  if (page.value && props.playable && !episode.unavailable)
    emit('play', page.value.revision, episode.id, doubleClick);
}
const programMenu = ref<InstanceType<typeof PodcastProgramMenu>>();
function openProgramMenu(event: MouseEvent | KeyboardEvent, voice: PodcastRecentEpisode) {
  if (!page.value || !(event.currentTarget instanceof HTMLElement)) return;
  const node = event.currentTarget;
  const anchor =
    node instanceof HTMLButtonElement
      ? node
      : node.querySelector<HTMLButtonElement>('[data-podcast-recent-more]');
  if (!anchor) return;
  const point =
    event instanceof MouseEvent && event.button === 2
      ? { x: event.clientX, y: event.clientY }
      : undefined;
  void programMenu.value?.open(voice, anchor, point);
}
function programKeyboard(event: KeyboardEvent, voice: PodcastRecentEpisode) {
  if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
  event.preventDefault();
  event.stopPropagation();
  openProgramMenu(event, voice);
}
async function focusRadio(voiceId: string) {
  // Scroll restoration changes the mounted window; update it before seeking the row.
  update();
  await nextTick();
  container.value
    ?.querySelector<HTMLButtonElement>(`[data-recent-voice="${CSS.escape(voiceId)}"] .recent-radio`)
    ?.focus({ preventScroll: true });
}
defineExpose({ focusRadio });
onMounted(props.model.open);
</script>

<template>
  <section class="podcast-recent" aria-label="最近更新单集" data-podcast-recent>
    <div v-if="issue && !episodes.length" class="error-state" role="alert">
      无法读取最近更新单集
      <button class="text-button" @click="model.retry">重试</button>
    </div>
    <div v-else-if="!page && pending" class="empty-state" role="status">
      <span class="spinner" />
      <p>正在读取最近更新单集…</p>
    </div>
    <div v-else-if="page && !episodes.length && !page.hasMore" class="empty-state">
      <Icon name="headphones" />
      <p>暂无最近更新的单集</p>
    </div>
    <div v-if="episodes.length" class="recent-columns recent-header" aria-hidden="true">
      <span>#</span><span>标题</span><span>播放量</span><span>时长</span>
    </div>
    <div
      v-if="episodes.length"
      ref="container"
      class="recent-rows"
      tabindex="-1"
      role="list"
      aria-label="最近更新单集列表"
      :style="{ height: episodes.length * height + 'px' }"
    >
      <article
        v-for="(voice, offset) in visible"
        :key="voice.id"
        class="recent-columns recent-episode"
        :class="{
          current: currentId === voice.id,
          striped: (start + offset) % 2 === 0,
          'recent-episode-compact': compact,
        }"
        :data-recent-index="start + offset"
        :data-recent-voice="voice.id"
        :style="{ top: (start + offset) * height + 'px', height: height + 'px' }"
        role="listitem"
        tabindex="0"
        aria-haspopup="menu"
        :aria-setsize="episodes.length"
        :aria-posinset="start + offset + 1"
        @contextmenu.stop.prevent="openProgramMenu($event, voice)"
        @keydown="programKeyboard($event, voice)"
        @dblclick="play(voice, true)"
      >
        <button
          class="icon-button recent-play"
          :aria-label="(currentId === voice.id && playing ? '暂停 ' : '播放 ') + voice.name"
          :disabled="!playable || voice.unavailable"
          @click.stop="play(voice, false)"
          @dblclick.stop
        >
          <span aria-hidden="true">{{ start + offset + 1 }}</span>
          <Icon :name="currentId === voice.id && playing ? 'pause' : 'play'" />
        </button>
        <div class="recent-title">
          <img v-if="voice.coverUrl" :src="voice.coverUrl" alt="" referrerpolicy="no-referrer" />
          <div v-else class="recent-art"><Icon name="headphones" /></div>
          <div class="recent-title-text">
            <strong :title="voice.name">{{ voice.name }}</strong>
            <div class="recent-secondary">
              <button
                v-if="voice.radioId && voice.radioName"
                class="text-button recent-radio"
                :title="voice.radioName"
                @click.stop="emit('radio', voice.radioId, voice.id)"
                @dblclick.stop
              >
                {{ voice.radioName }}
              </button>
              <span v-else-if="voice.radioName || voice.creator">
                {{ voice.radioName || voice.creator }}
              </span>
              <span v-if="voice.unavailable">不可播放</span>
              <span v-else-if="voice.listened">已听完</span>
              <span v-else-if="voice.listenLocationMs">
                已听 {{ time(voice.listenLocationMs / 1000) }}
              </span>
              <span v-if="voice.feeType">付费内容</span>
            </div>
          </div>
          <button
            type="button"
            class="icon-button recent-program-more"
            data-podcast-recent-more
            :aria-label="`更多：${voice.name}`"
            aria-haspopup="menu"
            aria-expanded="false"
            @click.stop="openProgramMenu($event, voice)"
            @dblclick.stop
          >
            <Icon name="more" />
          </button>
        </div>
        <span>{{ voice.listenerCount === null ? '—' : count.format(voice.listenerCount) }}</span>
        <span>{{ time(voice.durationMs / 1000) }}</span>
      </article>
    </div>
    <footer
      v-if="page?.hasMore || page?.capped || (issue && episodes.length)"
      class="recent-footer"
    >
      <template v-if="issue">
        <span role="alert">无法继续读取，已保留当前列表。</span>
        <button class="secondary" @click="model.retry">重试</button>
      </template>
      <span v-else-if="page?.capped" role="status">已达到本次读取上限。</span>
      <button v-else class="secondary" :disabled="pending" @click="model.next">
        {{ pending ? '正在读取…' : '加载更多' }}
      </button>
    </footer>
    <PodcastProgramMenu
      ref="programMenu"
      :page="page"
      :playable="playable"
      :queueable="queueable"
      :queue-recent="queueProgram"
      @recent-play="(epoch, target) => emit('programPlay', epoch, target)"
      @comments="(id, title) => emit('comments', id, title)"
      @download="(voices) => emit('download', voices)"
    />
  </section>
</template>

<style scoped>
.recent-columns {
  display: grid;
  grid-template-columns: 48px minmax(0, 1fr) 72px 64px;
  align-items: center;
  column-gap: 12px;
  color: var(--muted);
  font-size: 12px;
}
.recent-header {
  min-height: 32px;
}
.recent-header > :first-child {
  text-align: center;
}
.recent-rows {
  position: relative;
}
.recent-episode {
  position: absolute;
  inset-inline: 0;
  border-radius: 8px;
  --recent-cover-size: 48px;
  --recent-cover-radius: 8px;
  --recent-title-size: 16px;
}
.recent-episode-compact {
  --recent-cover-size: 36px;
  --recent-cover-radius: 4px;
  --recent-title-size: 14px;
}
.recent-episode.striped {
  background: var(--surface-alt);
}
.recent-episode:hover,
.recent-episode:focus-within {
  background: var(--hover);
}
.recent-episode.current .recent-title strong {
  color: var(--accent);
}
.recent-play {
  justify-self: center;
}
.recent-play .icon {
  display: none;
}
.recent-episode:is(:hover, :focus-within, .current) .recent-play:not(:disabled) .icon {
  display: block;
}
.recent-episode:is(:hover, :focus-within, .current) .recent-play:not(:disabled) span {
  display: none;
}
.recent-title {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 12px;
}
.recent-title img,
.recent-art {
  flex: 0 0 var(--recent-cover-size);
  width: var(--recent-cover-size);
  height: var(--recent-cover-size);
  object-fit: cover;
  border-radius: var(--recent-cover-radius);
}
.recent-art {
  display: grid;
  place-items: center;
  color: var(--accent);
  background: var(--surface);
}
.recent-title-text {
  flex: 1;
  min-width: 0;
}
.recent-title strong {
  display: block;
  color: var(--text);
  font-size: var(--recent-title-size);
}
.recent-title strong,
.recent-secondary > * {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.recent-secondary {
  display: flex;
  align-items: center;
  gap: 8px;
}
.recent-radio {
  display: inline;
  flex: 0 1 auto;
  color: var(--muted);
  min-width: 0;
  padding: 0;
}
.recent-program-more {
  flex: 0 0 32px;
  width: 32px;
  height: 32px;
}
@media (hover: hover) {
  .recent-episode:not(:hover, :focus-within) .recent-program-more {
    opacity: 0;
    pointer-events: none;
  }
}
.recent-footer {
  padding: 16px 0;
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 12px;
}
@media (max-width: 640px) {
  .recent-columns {
    grid-template-columns: 36px minmax(0, 1fr) 54px 46px;
    column-gap: 7px;
  }
  .recent-title {
    gap: 7px;
  }
}
</style>
