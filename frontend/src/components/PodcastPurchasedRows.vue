<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useOuterVirtualRows } from '../composables/useOuterVirtualRows';
import type { PodcastModel } from '../composables/usePodcasts';
import type { EpisodeView, PodcastPurchasedRow, PodcastPurchasedTarget } from '../services/podcast';
import { time } from '../services/player';
import Icon from './Icon.vue';

const props = defineProps<{ model: PodcastModel; playable: boolean }>();
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
const rowHeight = 64;
const { container, start, end } = useOuterVirtualRows(
  () => rows.value.length,
  rowHeight,
  (list) => list?.closest<HTMLElement>('.podcast-page') ?? undefined,
  (element) => Number(element.closest<HTMLElement>('[data-purchase-index]')?.dataset.purchaseIndex),
);
const visibleRows = computed(() => rows.value.slice(start.value, end.value));
const loading = computed(() => props.model.loading.value);
const key = (row: PodcastPurchasedRow) => `${row.kind}:${row.item.id}`;

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
    ref="container"
    class="purchase-list"
    tabindex="-1"
    role="list"
    aria-label="已购播单和单集"
  >
    <div aria-hidden="true" :style="{ height: `${start * rowHeight}px` }" />
    <article
      v-for="(row, index) in visibleRows"
      :key="key(row)"
      role="listitem"
      class="purchase-row"
      :class="{ selected: selected === key(row) }"
      :data-purchased-kind="row.kind"
      :data-purchased-id="row.item.id"
      :data-purchase-index="start + index"
      :aria-posinset="start + index + 1"
      :aria-setsize="rows.length"
      @dblclick="playRow(row, $event)"
    >
      <img v-if="row.item.coverUrl" :src="row.item.coverUrl" alt="" referrerpolicy="no-referrer" />
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
      <span v-if="row.kind === 'radio'" class="muted">{{ row.item.programCount }} 期</span>
      <template v-else>
        <span class="muted">{{ time(row.item.durationMs / 1000) }}</span>
        <button
          type="button"
          class="icon-button"
          aria-label="下载单集"
          :disabled="loading || row.item.unavailable"
          @click="download(row)"
        >
          <Icon name="down" />
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
    </article>
    <div aria-hidden="true" :style="{ height: `${(rows.length - end) * rowHeight}px` }" />
  </div>
  <button
    v-if="typeof model.page.value?.purchases?.nextOffset === 'number'"
    type="button"
    class="text-button purchase-load-more"
    :disabled="loading"
    @click="model.loadMorePurchases"
  >
    加载更多
  </button>
</template>

<style scoped>
.purchase-row {
  height: 64px;
  box-sizing: border-box;
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr) max-content 32px 32px;
  gap: 8px;
  align-items: center;
  padding-inline: 8px;
}
.purchase-row.selected {
  background: var(--hover);
}
.purchase-row img,
.purchase-placeholder {
  width: 44px;
  height: 44px;
  border-radius: 4px;
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
}
.purchase-load-more {
  display: block;
  margin: 12px auto;
}
</style>
