<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useOuterVirtualRows } from '../composables/useOuterVirtualRows';
import type { PodcastModel } from '../composables/usePodcasts';
import type { EpisodeView, PodcastPurchasedRow, PodcastPurchasedTarget } from '../services/podcast';
import { playlistTableCount } from '../services/playlistHeader';
import Icon from './Icon.vue';

const props = defineProps<{
  model: PodcastModel;
  playable: boolean;
  compact: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  comments: [id: string, title: string];
  download: [episode: EpisodeView, live: () => boolean, source: PodcastPurchasedTarget];
  play: [epoch: string, target: PodcastPurchasedTarget, live: () => boolean];
}>();
const rows = computed(() => props.model.page.value?.purchases?.rows ?? []);
const selected = ref('');
watch(rows, () => {
  if (!rows.value.some((row) => key(row) === selected.value)) selected.value = '';
});
const rowHeight = computed(() => (props.compact ? 56 : 64));
const { container, start, end } = useOuterVirtualRows(
  () => rows.value.length,
  () => rowHeight.value,
  (list) => list?.closest<HTMLElement>('.podcast-page') ?? undefined,
  (element) => Number(element.closest<HTMLElement>('[data-purchase-index]')?.dataset.purchaseIndex),
);
const visibleRows = computed(() => rows.value.slice(start.value, end.value));
const loading = computed(() => props.model.loading.value);
const key = (row: PodcastPurchasedRow) => `${row.kind}:${row.item.id}`;

function playCount(row: PodcastPurchasedRow) {
  if (row.kind !== 'radio') return null;
  const count = row.item.playCount;
  return typeof count === 'string' ? count : (playlistTableCount(count, false)?.text ?? null);
}

async function open(row: PodcastPurchasedRow, parent = false) {
  const checked = await props.model.resolvePurchasedRow(row);
  if (!checked || checked.item.unavailable) return;
  const radio = checked.kind === 'radio' ? checked.item.id : parent ? checked.item.radioId : null;
  if (radio) props.model.openCard(radio);
}
async function comments(row: PodcastPurchasedRow) {
  const checked = await props.model.resolvePurchasedRow(row);
  if (checked?.kind === 'episode' && !checked.item.unavailable)
    emit('comments', checked.item.id, checked.item.name);
}
function download(row: PodcastPurchasedRow) {
  if (row.kind !== 'episode' || row.item.unavailable) return;
  const source = props.model.capturePurchasedRow(row);
  if (source) emit('download', row.item, source.live, source.target);
}
function play(row: PodcastPurchasedRow, event: MouseEvent | KeyboardEvent) {
  if (!props.playable || row.kind !== 'episode' || row.item.unavailable) return;
  const source = props.model.capturePurchasedRow(row);
  if (!source) return;
  event.preventDefault();
  selected.value = key(row);
  emit('play', source.epoch, source.target, source.live);
}
function playRow(row: PodcastPurchasedRow, event: MouseEvent) {
  if (event.target instanceof Element && event.target.closest('button, a, input')) return;
  play(row, event);
}
</script>

<template>
  <div
    v-if="rows.length"
    class="purchase-table"
    :class="{ compact }"
    role="table"
    aria-label="已购播单和单集"
    :aria-rowcount="rows.length + 1"
    aria-colcount="5"
    :style="{ '--purchase-row-height': `${rowHeight}px` }"
  >
    <div class="purchase-grid purchase-header muted" role="row" aria-rowindex="1">
      <span class="purchase-index" role="columnheader" aria-label="序号">#</span>
      <span role="columnheader">标题</span>
      <span role="columnheader">播放量</span>
      <span role="columnheader">声音数</span>
      <span role="columnheader"><span class="sr-only">操作</span></span>
    </div>
    <div ref="container" class="purchase-list" tabindex="-1" role="rowgroup">
      <div aria-hidden="true" :style="{ height: `${start * rowHeight}px` }" />
      <article
        v-for="(row, index) in visibleRows"
        :key="key(row)"
        role="row"
        class="purchase-grid purchase-row"
        :class="{
          selected: selected === key(row),
          current: row.kind === 'episode' && row.item.id === currentId,
        }"
        :data-purchased-kind="row.kind"
        :data-purchased-id="row.item.id"
        :data-purchase-index="start + index"
        :aria-rowindex="start + index + 2"
        @dblclick="playRow(row, $event)"
      >
        <span role="cell" class="purchase-index muted">{{
          String(start + index + 1).padStart(2, '0')
        }}</span>
        <div class="purchase-name" role="cell">
          <img
            v-if="row.item.coverUrl"
            :src="row.item.coverUrl"
            alt=""
            referrerpolicy="no-referrer"
          />
          <span v-else class="purchase-placeholder"><Icon name="music" /></span>
          <div class="purchase-title">
            <button
              v-if="row.kind === 'radio'"
              type="button"
              class="text-button ellipsis"
              :disabled="loading || row.item.unavailable"
              @click="open(row)"
            >
              {{ row.item.name }}
            </button>
            <button
              v-else
              type="button"
              class="text-button ellipsis"
              :aria-pressed="selected === key(row)"
              :aria-current="row.item.id === currentId ? 'true' : undefined"
              :aria-keyshortcuts="playable && !row.item.unavailable ? 'Enter' : undefined"
              :disabled="loading"
              @click="selected = key(row)"
              @dblclick.stop="play(row, $event)"
              @keydown.enter="play(row, $event)"
            >
              {{ row.item.name }}
            </button>
            <button
              v-if="row.kind === 'episode' && row.item.radioId"
              type="button"
              class="text-button muted ellipsis"
              data-purchased-parent
              :disabled="loading || row.item.unavailable"
              @click="open(row, true)"
            >
              {{ row.item.radioName || '所属播客' }}
            </button>
            <span v-else-if="row.item.creator" class="muted ellipsis">{{ row.item.creator }}</span>
          </div>
        </div>
        <span role="cell" class="muted ellipsis" data-purchased-play-count>{{
          playCount(row)
        }}</span>
        <span role="cell" class="muted ellipsis" data-purchased-program-count>
          {{ row.kind === 'radio' ? `${row.item.programCount}期` : null }}
        </span>
        <div class="purchase-actions" role="cell">
          <template v-if="row.kind === 'episode'">
            <button
              type="button"
              class="icon-button"
              aria-label="下载单集"
              :disabled="loading || row.item.unavailable"
              @click="download(row)"
            >
              <Icon name="download" />
            </button>
            <button
              type="button"
              class="icon-button"
              aria-label="单集评论"
              :disabled="loading || row.item.unavailable"
              @click="comments(row)"
            >
              <Icon name="comment" />
            </button>
          </template>
        </div>
      </article>
      <div aria-hidden="true" :style="{ height: `${(rows.length - end) * rowHeight}px` }" />
    </div>
  </div>
  <div v-if="typeof model.page.value?.purchases?.nextOffset === 'number'" class="purchase-footer">
    <button type="button" class="text-button" :disabled="loading" @click="model.loadMorePurchases">
      加载更多 <Icon name="down" />
    </button>
  </div>
</template>

<style scoped>
.purchase-grid {
  box-sizing: border-box;
  display: grid;
  grid-template-columns:
    clamp(28px, 7%, 45px) minmax(0, 1fr) clamp(54px, 13%, 92px)
    clamp(54px, 17%, 120px) 72px;
  column-gap: 8px;
  align-items: center;
  padding-inline: 8px;
}
.purchase-table {
  --purchase-cover-size: 48px;
  --purchase-cover-radius: 8px;
}
.purchase-table.compact {
  --purchase-cover-size: 36px;
  --purchase-cover-radius: 4px;
}
.purchase-header {
  min-height: 40px;
  margin-bottom: 8px;
  font-size: 13px;
  border-bottom: 1px solid var(--line);
}
.compact .purchase-header {
  margin-bottom: 0;
}
.purchase-index {
  text-align: center;
}
.purchase-row {
  height: var(--purchase-row-height);
}
.purchase-name {
  display: grid;
  grid-template-columns: var(--purchase-cover-size) minmax(0, 1fr);
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.purchase-actions {
  display: grid;
  grid-template-columns: repeat(2, 32px);
  gap: 8px;
}
.purchase-row.selected {
  background: var(--hover);
}
.purchase-row img,
.purchase-placeholder {
  width: var(--purchase-cover-size);
  height: var(--purchase-cover-size);
  border-radius: var(--purchase-cover-radius);
}
.purchase-placeholder {
  display: grid;
  place-items: center;
}
.purchase-title {
  display: grid;
  justify-items: start;
  min-width: 0;
}
.purchase-title .text-button {
  max-width: 100%;
  padding: 0;
  text-align: start;
  color: var(--text);
}
.purchase-title .text-button[data-purchased-parent] {
  color: var(--muted);
}
.purchase-row.current .purchase-title .text-button,
.purchase-title .text-button:hover:not(:disabled) {
  color: var(--accent);
}
.purchase-title > :first-child {
  font-size: 16px;
}
.compact .purchase-title > :first-child {
  font-size: 14px;
}
.purchase-footer {
  display: flex;
  justify-content: center;
  margin-top: 12px;
  padding: 12px;
  border-top: 1px solid var(--line);
  font-size: 14px;
}
.purchase-footer button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.purchase-footer svg {
  width: 14px;
  height: 14px;
}
</style>
