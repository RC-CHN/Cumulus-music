<script setup lang="ts">
import { computed, nextTick, ref, watch, useId } from 'vue';
import type { TrackView } from '../../services/contracts';
import { useVirtualRows } from '../../composables/useVirtualRows';
import { time } from '../../services/player';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
import PlaylistBatchResize from './PlaylistBatchResize.vue';
import { usePlaylistBatchColumns } from '../../composables/usePlaylistBatchColumns';
import { batchSelectionNeighbor } from '../../services/playlistBatchTable';
const props = defineProps<{
  tracks: TrackView[];
  selected: Set<string>;
  liked: Set<string>;
  disabled: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  toggle: [index: number, range: boolean];
  select: [index: number];
  all: [force?: boolean];
}>();
const { container, start, end, onScroll } = useVirtualRows(() => props.tracks.length, 64);
const columns = usePlaylistBatchColumns(container, () => props.disabled);
const bodyId = useId();
const rows = computed(() => props.tracks.slice(start.value, end.value));
const ids = computed(() => props.tracks.map((t) => t.id));
const count = computed(() => new Set(props.tracks.map((t) => t.id)).size);
const active = ref(0);
let recycling = false;
watch(
  [start, end],
  ([from, to]) => {
    const row = document.activeElement?.closest<HTMLElement>('[data-batch-index]');
    if (!row || !container.value?.contains(row)) return;
    const index = Number(row.dataset.batchIndex);
    if (index < from || index >= to) {
      active.value = Math.min(props.tracks.length - 1, Math.ceil(container.value.scrollTop / 64));
      recycling = true;
      container.value.focus({ preventScroll: true });
      recycling = false;
    }
  },
  { flush: 'pre' },
);
function toggle(event: MouseEvent, index: number) {
  if (props.disabled) return;
  (event.currentTarget as HTMLElement)
    .closest<HTMLElement>('[data-batch-index]')
    ?.focus({ preventScroll: true });
  emit('toggle', index, event.shiftKey);
}
async function key(event: KeyboardEvent, index: number) {
  if (props.disabled || event.altKey) return;
  if (event.key === ' ' || event.key === 'Enter') {
    event.preventDefault();
    event.stopPropagation();
    emit('toggle', index, event.shiftKey);
    return;
  }
  let target = index;
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    const direction = event.key === 'ArrowDown' ? 1 : -1;
    if (!event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
      const neighbor = batchSelectionNeighbor(ids.value, props.selected, direction);
      event.preventDefault();
      event.stopPropagation();
      if (neighbor === undefined) return;
      target = neighbor;
    } else target += direction;
  } else if (event.key === 'Home') target = 0;
  else if (event.key === 'End') target = props.tracks.length - 1;
  else return;
  event.preventDefault();
  event.stopPropagation();
  target = Math.max(0, Math.min(props.tracks.length - 1, target));
  if (event.shiftKey) emit('toggle', target, true);
  else if (!event.ctrlKey && !event.metaKey) emit('select', target);
  await focusIndex(target);
}
async function focusBody(event: FocusEvent) {
  if (recycling || props.disabled || !event.relatedTarget) return;
  const selected = ids.value.findIndex((id) => props.selected.has(id));
  if (selected < 0) emit('select', 0);
  await focusIndex(Math.max(0, selected));
}
async function focusIndex(target: number) {
  active.value = target;
  if (container.value) {
    const top = target * 64;
    if (top < container.value.scrollTop) container.value.scrollTop = top;
    else if (top + 64 > container.value.scrollTop + container.value.clientHeight)
      container.value.scrollTop = top + 64 - container.value.clientHeight;
    onScroll();
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    await nextTick();
    container.value
      ?.querySelector<HTMLElement>(`[data-batch-index="${target}"]`)
      ?.focus({ preventScroll: true });
  }
}
function shortcut(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
    event.preventDefault();
    event.stopPropagation();
    if (!props.disabled) emit('all', true);
  }
}
</script>
<template>
  <div
    class="playlist-batch-table"
    role="table"
    :aria-rowcount="tracks.length + 1"
    @keydown="shortcut"
  >
    <div class="batch-columns batch-table-head" role="row" :style="columns.style.value">
      <span role="columnheader"
        ><input
          type="checkbox"
          aria-label="全选歌曲"
          :checked="count > 0 && selected.size === count"
          :indeterminate="selected.size > 0 && selected.size < count"
          :disabled="disabled || !count"
          @change="emit('all')"
      /></span>
      <span role="columnheader" class="batch-title-heading">
        <button class="batch-select-label" :disabled="disabled" @click="emit('all')">
          {{ selected.size ? `已选 ${selected.size} 首歌曲` : `全选（共${tracks.length}首）` }}
        </button>
        <PlaylistBatchResize :model="columns" :column="0" label="歌曲" :controls="bodyId" />
      </span>
      <span role="columnheader"
        >专辑<PlaylistBatchResize
          :model="columns"
          :column="1"
          label="专辑"
          :controls="bodyId" /></span
      ><span role="columnheader">喜欢</span><span role="columnheader">时长</span>
    </div>
    <div
      ref="container"
      :id="bodyId"
      class="batch-table-scroll"
      tabindex="0"
      @focus.self="focusBody"
      @scroll.passive="onScroll"
      @keydown.self="key($event, active)"
    >
      <div :style="{ height: `${start * 64}px` }" aria-hidden="true" />
      <div
        v-for="(track, i) in rows"
        :key="`${start + i}:${track.id}`"
        class="batch-columns batch-track-row"
        :style="columns.style.value"
        :class="{
          selected: selected.has(track.id),
          current: !track.local && !track.missing && currentId === track.id,
          unavailable: track.missing,
        }"
        role="row"
        :aria-rowindex="start + i + 2"
        :aria-selected="selected.has(track.id)"
        :aria-disabled="disabled"
        :data-batch-index="start + i"
        tabindex="-1"
        @focus="active = start + i"
        @click="toggle($event, start + i)"
        @keydown="key($event, start + i)"
        @dblclick.prevent
      >
        <span role="cell"
          ><input
            type="checkbox"
            tabindex="-1"
            :aria-label="`选择 ${track.name}`"
            :checked="selected.has(track.id)"
            :disabled="disabled"
            @click.stop="toggle($event, start + i)"
        /></span>
        <span class="batch-track-title" role="cell"
          ><img
            v-if="track.coverUrl"
            :src="artworkAt(track.coverUrl, 120)"
            alt=""
            loading="lazy"
            decoding="async"
            referrerpolicy="no-referrer"
          /><span v-else class="batch-track-art"><Icon name="music" /></span
          ><span class="batch-track-copy">
            <span class="batch-track-name ellipsis" :data-tooltip="track.name">{{
              track.name
            }}</span>
            <span class="batch-track-subtitle"
              ><small v-if="track.fee === 1" class="vip-tag">VIP</small
              ><span class="ellipsis" :data-tooltip="track.artists.join(' / ')">{{
                track.artists.join(' / ') || (track.local ? '本地文件' : '未知歌手')
              }}</span></span
            >
          </span></span
        >
        <span role="cell" class="ellipsis" :data-tooltip="track.album">{{ track.album }}</span
        ><span
          role="cell"
          class="batch-like-status"
          :class="{ liked: !track.local && liked.has(track.id) }"
        >
          <span role="img" :aria-label="!track.local && liked.has(track.id) ? '已喜欢' : '未喜欢'"
            ><Icon name="heart"
          /></span> </span
        ><span role="cell">{{ track.missing ? '—' : time(track.durationMs / 1000) }}</span>
      </div>
      <div :style="{ height: `${(tracks.length - end) * 64}px` }" aria-hidden="true" />
    </div>
  </div>
</template>
