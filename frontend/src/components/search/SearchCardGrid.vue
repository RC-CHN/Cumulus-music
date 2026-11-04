<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { SearchItem } from '../../services/search';
import { searchCardKey } from '../../services/searchCards';
import {
  initialPlaylistShares,
  playlistBaseWidth,
  readPlaylistColumnShares,
  resizePlaylistColumns,
  searchPlaylistColumns,
  writePlaylistColumnShares,
  type PlaylistColumnHandle,
} from '../../services/searchPlaylistColumns';
import { rowWindow } from '../../services/virtualWindow';
import SearchPreviewCarousel from './SearchPreviewCarousel.vue';
const props = defineProps<{
  items: SearchItem[];
  scroller?: HTMLElement;
  virtual?: boolean;
  preview?: boolean;
  carousel?: { label: string; compact: boolean; position: number };
  playlistTable?: boolean;
}>();
const emit = defineEmits<{ position: [index: number] }>();
const PLAYLIST_ROW_HEIGHT = 64;
const positionedRows = computed(() => props.playlistTable && props.virtual);
const grid = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const playlistWidth = ref(0);
const playlistShares = shallowRef(initialPlaylistShares);
watch(
  () => props.playlistTable,
  (enabled) => {
    if (enabled) playlistShares.value = readPlaylistColumnShares();
  },
  { immediate: true },
);
const playlistSurplus = computed(() => Math.max(0, playlistWidth.value - playlistBaseWidth));
const playlistTracks = computed(() =>
  searchPlaylistColumns
    .map(({ width }, index) =>
      index
        ? `calc(${width}px + (100% - ${playlistBaseWidth}px) * ${playlistShares.value[index - 1]})`
        : `${width}px`,
    )
    .join(' '),
);
let drag:
  | {
      index: PlaylistColumnHandle;
      pointer: number;
      last: number;
      pending: number;
      before: typeof initialPlaylistShares;
    }
  | undefined;
let dragFrame = 0;
function columnWidth(index: PlaylistColumnHandle, max = false) {
  const base = searchPlaylistColumns[index + 1]!.width;
  const share = max
    ? playlistShares.value.slice(index).reduce((sum, value) => sum + value, 0)
    : playlistShares.value[index];
  return Math.round(base + share * playlistSurplus.value);
}
function resizePlaylistColumn(index: PlaylistColumnHandle, pixels: number) {
  playlistShares.value = resizePlaylistColumns(
    playlistShares.value,
    index,
    pixels,
    playlistSurplus.value,
  );
}
function applyDrag() {
  if (!drag) return;
  resizePlaylistColumn(drag.index, drag.pending - drag.last);
  drag.last = drag.pending;
}
function beginResize(event: PointerEvent, index: PlaylistColumnHandle) {
  if (event.button !== 0 || !event.isPrimary || drag || !playlistSurplus.value) return;
  const handle = event.currentTarget as HTMLElement;
  handle.focus({ preventScroll: true });
  handle.setPointerCapture(event.pointerId);
  drag = {
    index,
    pointer: event.pointerId,
    last: event.clientX,
    pending: event.clientX,
    before: playlistShares.value,
  };
}
function moveResize(event: PointerEvent) {
  if (!drag || event.pointerId !== drag.pointer) return;
  drag.pending = event.clientX;
  if (!dragFrame)
    dragFrame = requestAnimationFrame(() => {
      dragFrame = 0;
      applyDrag();
    });
}
function finishResize(event: PointerEvent) {
  if (!drag || event.pointerId !== drag.pointer) return;
  cancelAnimationFrame(dragFrame);
  dragFrame = 0;
  if (event.type === 'pointerup') {
    drag.pending = event.clientX;
    applyDrag();
    if (playlistShares.value !== drag.before) writePlaylistColumnShares(playlistShares.value);
  } else {
    playlistShares.value = drag.before;
  }
  drag = undefined;
}
function keyResize(event: KeyboardEvent, index: PlaylistColumnHandle) {
  const delta =
    event.key === 'ArrowLeft'
      ? -12
      : event.key === 'ArrowRight'
        ? 12
        : event.key === 'Home'
          ? -playlistSurplus.value
          : event.key === 'End'
            ? playlistSurplus.value
            : 0;
  if (!delta || drag) return;
  event.preventDefault();
  const before = playlistShares.value;
  resizePlaylistColumn(index, delta);
  if (playlistShares.value !== before) writePlaylistColumnShares(playlistShares.value);
}
const columns = ref(1),
  pitch = ref(260);
const range = shallowRef({ start: 0, end: 5 });
let observer: ResizeObserver | undefined,
  frame = 0;
let scroller: HTMLElement | undefined;
const count = computed(() => Math.ceil(props.items.length / columns.value));
const start = computed(() => (props.virtual ? range.value.start * columns.value : 0));
const end = computed(() =>
  props.virtual
    ? Math.min(props.items.length, range.value.end * columns.value)
    : props.preview
      ? Math.min(props.items.length, columns.value)
      : props.items.length,
);
const visible = computed(() => props.items.slice(start.value, end.value));
const top = computed(() =>
  props.virtual && !positionedRows.value && range.value.start
    ? range.value.start * pitch.value - 20
    : 0,
);
const bottom = computed(() =>
  props.virtual && !positionedRows.value && range.value.end < count.value
    ? (count.value - range.value.end) * pitch.value - 20
    : 0,
);
function gridOffset() {
  return grid.value && scroller
    ? grid.value.getBoundingClientRect().top -
        scroller.getBoundingClientRect().top +
        scroller.scrollTop
    : 0;
}
function update(measure = false) {
  if (!grid.value || !scroller || (!props.virtual && !props.preview)) return;
  const oldColumns = columns.value,
    oldPitch = pitch.value;
  const focused =
    document.activeElement instanceof HTMLElement && grid.value.contains(document.activeElement)
      ? document.activeElement.closest<HTMLElement>('[data-grid-index]')
      : null;
  columns.value = props.playlistTable
    ? 1
    : Math.max(1, getComputedStyle(grid.value).gridTemplateColumns.split(' ').length);
  if (props.preview) {
    if (focused && Number(focused.dataset.gridIndex) >= columns.value) {
      void nextTick(() => {
        if (document.activeElement !== document.body) return;
        const targets = grid.value?.querySelectorAll<HTMLButtonElement>(
          '.search-card-open:not(:disabled)',
        );
        const target =
          targets?.[targets.length - 1] ??
          grid.value
            ?.closest('.search-result-section')
            ?.querySelector<HTMLButtonElement>('.search-section-link');
        target?.focus({ preventScroll: true });
        target?.scrollIntoView({ block: 'nearest' });
      });
    }
    return;
  }
  const offset = gridOffset(),
    before = Math.max(0, scroller.scrollTop - offset);
  const anchor = Math.floor(before / oldPitch) * oldColumns;
  const card = grid.value.querySelector<HTMLElement>('.search-result-card');
  if (props.playlistTable) pitch.value = PLAYLIST_ROW_HEIGHT;
  else if (card) pitch.value = card.getBoundingClientRect().height + 20;
  if (
    measure &&
    before > 0 &&
    (oldColumns !== columns.value || Math.abs(oldPitch - pitch.value) > 1)
  )
    scroller.scrollTop =
      offset + Math.floor(anchor / columns.value) * pitch.value + (before % oldPitch);
  range.value = rowWindow(
    count.value,
    pitch.value,
    scroller.clientHeight,
    Math.max(0, scroller.scrollTop - offset),
    range.value,
    2,
  );
}
function scroll() {
  if (!frame)
    frame = requestAnimationFrame(() => {
      frame = 0;
      update();
    });
}
watch(
  () =>
    [
      grid.value,
      heading.value,
      props.scroller,
      props.virtual,
      props.preview,
      props.playlistTable,
    ] as const,
  () => {
    scroller?.removeEventListener('scroll', scroll);
    observer?.disconnect();
    scroller = props.scroller;
    playlistWidth.value = heading.value?.clientWidth ?? 0;
    if (!grid.value || (!props.virtual && !props.preview && !props.playlistTable)) return;
    if (props.virtual) scroller?.addEventListener('scroll', scroll, { passive: true });
    observer = new ResizeObserver(() => {
      playlistWidth.value = heading.value?.clientWidth ?? 0;
      update(true);
    });
    if (heading.value) observer.observe(heading.value);
    if (props.virtual || props.preview) {
      observer.observe(grid.value);
      if (scroller) observer.observe(scroller);
    }
    update();
  },
  { flush: 'post' },
);
watch(
  () => props.items.length,
  () => update(),
  { flush: 'post' },
);
async function focusItem(index: number, last = false, primary = false) {
  if (!scroller) return;
  const row = Math.floor(index / columns.value),
    target = gridOffset() + row * pitch.value,
    headerHeight = heading.value?.offsetHeight ?? 0;
  if (target < scroller.scrollTop + headerHeight) scroller.scrollTop = target - headerHeight;
  else if (target + pitch.value > scroller.scrollTop + scroller.clientHeight)
    scroller.scrollTop = target + pitch.value - scroller.clientHeight;
  update();
  await nextTick();
  const buttons = grid.value
    ?.querySelector(`[data-grid-index="${index}"]`)
    ?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
  const opener =
    primary && props.playlistTable
      ? [...(buttons ?? [])].find((button) => button.matches('.search-card-open'))
      : undefined;
  (opener ?? (last ? buttons?.[buttons.length - 1] : buttons?.[0]))?.focus({
    preventScroll: true,
  });
}
function keydown(event: KeyboardEvent) {
  if (!props.virtual || !(event.target instanceof HTMLElement)) return;
  const cell = event.target.closest<HTMLElement>('[data-grid-index]');
  if (!cell) return;
  const index = Number(cell.dataset.gridIndex);
  if (event.key === 'Tab') {
    const buttons = [...cell.querySelectorAll('button:not(:disabled)')];
    const target = index + (event.shiftKey ? -1 : 1);
    const edge = event.shiftKey ? buttons[0] : buttons.at(-1);
    if (
      event.target === edge &&
      target >= 0 &&
      target < props.items.length &&
      (target < start.value || target >= end.value)
    ) {
      event.preventDefault();
      void focusItem(target, event.shiftKey);
    }
  } else if (event.target.matches('.search-card-open, .search-voice-card .search-card-play')) {
    const move: Record<string, number> = {
      ArrowRight: index + 1,
      ArrowLeft: index - 1,
      ArrowDown: index + columns.value,
      ArrowUp: index - columns.value,
      Home: 0,
      End: props.items.length - 1,
    };
    if (event.key in move) {
      event.preventDefault();
      void focusItem(Math.max(0, Math.min(props.items.length - 1, move[event.key]!)), false, true);
    }
  }
}
onBeforeUnmount(() => {
  scroller?.removeEventListener('scroll', scroll);
  observer?.disconnect();
  cancelAnimationFrame(frame);
  cancelAnimationFrame(dragFrame);
});
defineExpose({ reconcileViewport: () => update() });
</script>
<template>
  <SearchPreviewCarousel
    v-if="carousel"
    :items="items"
    v-bind="carousel"
    @position="emit('position', $event)"
    v-slot="{ item, index }"
  >
    <slot :item="item" :index="index" />
  </SearchPreviewCarousel>
  <div
    v-else
    :class="{ 'search-playlist-table': playlistTable }"
    :style="playlistTable ? { '--search-playlist-tracks': playlistTracks } : undefined"
    :role="playlistTable ? 'table' : undefined"
    :aria-label="playlistTable ? '搜索歌单结果' : undefined"
    :aria-rowcount="playlistTable ? items.length + 1 : undefined"
  >
    <div
      v-if="playlistTable"
      ref="heading"
      class="search-playlist-columns"
      role="row"
      aria-rowindex="1"
    >
      <span
        v-for="column in searchPlaylistColumns"
        :key="column.name"
        role="columnheader"
        :aria-label="'label' in column ? column.label : undefined"
      >
        <span>{{ column.name }}</span>
        <span
          v-if="'resizer' in column && playlistSurplus > 0"
          class="search-playlist-column-resizer"
          role="separator"
          aria-orientation="vertical"
          :aria-label="`调整${column.name}列宽`"
          :aria-valuemin="column.width"
          :aria-valuemax="columnWidth(column.resizer, true)"
          :aria-valuenow="columnWidth(column.resizer)"
          tabindex="0"
          @pointerdown.prevent="beginResize($event, column.resizer)"
          @pointermove="moveResize"
          @pointerup="finishResize"
          @pointercancel="finishResize"
          @lostpointercapture="finishResize"
          @keydown="keyResize($event, column.resizer)"
        />
      </span>
    </div>
    <div
      ref="grid"
      class="search-result-grid"
      :class="{
        'search-playlist-grid': playlistTable,
        'search-playlist-virtual': positionedRows,
        'search-person-grid': items.every((i) => i.kind === 'artist' || i.kind === 'user'),
        'search-related-grid': items.every((i) => i.kind === 'related'),
        'search-video-grid': items.every((i) => i.kind === 'mv'),
        'search-voice-grid': items.every((i) => i.kind === 'voice'),
      }"
      :role="playlistTable ? 'rowgroup' : undefined"
      :style="positionedRows ? { height: `${items.length * PLAYLIST_ROW_HEIGHT}px` } : undefined"
      :data-loaded-count="items.length"
      :data-preview="preview || undefined"
      @keydown="keydown"
    >
      <div
        v-if="top"
        aria-hidden="true"
        class="search-grid-spacer"
        :style="{ height: `${top}px` }"
      />
      <div
        v-for="(item, i) in visible"
        :key="searchCardKey(item)"
        class="search-grid-cell"
        :role="playlistTable ? 'presentation' : undefined"
        :data-grid-index="start + i"
        :style="positionedRows ? { top: `${(start + i) * PLAYLIST_ROW_HEIGHT}px` } : undefined"
      >
        <slot :item="item" :index="start + i" />
      </div>
      <div
        v-if="bottom"
        aria-hidden="true"
        class="search-grid-spacer"
        :style="{ height: `${bottom}px` }"
      />
    </div>
  </div>
</template>
