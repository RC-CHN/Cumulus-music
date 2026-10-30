<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { TrackView } from '../../services/contracts';
import { useVirtualRows } from '../../composables/useVirtualRows';
import { time } from '../../services/player';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
const props = defineProps<{
  tracks: TrackView[];
  selected: Set<string>;
  disabled: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  toggle: [index: number, range: boolean];
  select: [index: number];
  all: [force?: boolean];
}>();
const { container, start, end, onScroll } = useVirtualRows(() => props.tracks.length, 64);
const rows = computed(() => props.tracks.slice(start.value, end.value));
const count = computed(() => new Set(props.tracks.map((t) => t.id)).size);
const active = ref(0);
watch(
  [start, end],
  ([from, to]) => {
    const row = document.activeElement?.closest<HTMLElement>('[data-batch-index]');
    if (!row || !container.value?.contains(row)) return;
    const index = Number(row.dataset.batchIndex);
    if (index < from || index >= to) {
      active.value = Math.max(from, Math.min(to - 1, index));
      container.value.focus({ preventScroll: true });
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
  if (props.disabled) return;
  if (event.key === ' ' || event.key === 'Enter') {
    event.preventDefault();
    event.stopPropagation();
    emit('toggle', index, event.shiftKey);
    return;
  }
  let target = index;
  if (event.key === 'ArrowDown') target++;
  else if (event.key === 'ArrowUp') target--;
  else if (event.key === 'Home') target = 0;
  else if (event.key === 'End') target = props.tracks.length - 1;
  else return;
  event.preventDefault();
  event.stopPropagation();
  target = Math.max(0, Math.min(props.tracks.length - 1, target));
  active.value = target;
  if (event.shiftKey) emit('toggle', target, true);
  else if (!event.ctrlKey && !event.metaKey) emit('select', target);
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
    <div class="batch-columns batch-table-head" role="row">
      <span role="columnheader"
        ><input
          type="checkbox"
          aria-label="全选歌曲"
          :checked="count > 0 && selected.size === count"
          :indeterminate="selected.size > 0 && selected.size < count"
          :disabled="disabled || !count"
          @change="emit('all')"
      /></span>
      <button class="batch-select-label" :disabled="disabled" @click="emit('all')">
        {{ selected.size ? `已选 ${selected.size} 首歌曲` : `全选（共${tracks.length}首）` }}
      </button>
      <span role="columnheader">歌手</span><span role="columnheader">专辑</span
      ><span role="columnheader">时长</span>
    </div>
    <div
      ref="container"
      class="batch-table-scroll"
      tabindex="-1"
      @scroll.passive="onScroll"
      @keydown.self="key($event, active)"
    >
      <div :style="{ height: `${start * 64}px` }" aria-hidden="true" />
      <div
        v-for="(track, i) in rows"
        :key="`${start + i}:${track.id}`"
        class="batch-columns batch-track-row"
        :class="{
          selected: selected.has(track.id),
          current: currentId === track.id,
          unavailable: track.missing,
        }"
        role="row"
        :aria-rowindex="start + i + 2"
        :aria-selected="selected.has(track.id)"
        :aria-disabled="disabled"
        :data-batch-index="start + i"
        :tabindex="start + i === Math.max(start, Math.min(end - 1, active)) ? 0 : -1"
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
          ><span class="ellipsis" :data-tooltip="track.name"
            >{{ track.name }} <small v-if="track.fee === 1" class="vip-tag">VIP</small></span
          ></span
        >
        <span role="cell" class="ellipsis" :data-tooltip="track.artists.join(' / ')">{{
          track.artists.join(' / ') || (track.local ? '本地文件' : '未知歌手')
        }}</span
        ><span role="cell" class="ellipsis" :data-tooltip="track.album">{{ track.album }}</span
        ><span role="cell">{{ track.missing ? '—' : time(track.durationMs / 1000) }}</span>
      </div>
      <div :style="{ height: `${(tracks.length - end) * 64}px` }" aria-hidden="true" />
    </div>
  </div>
</template>
