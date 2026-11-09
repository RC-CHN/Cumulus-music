<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { SearchItem } from '../../services/search';
import { searchCardKey } from '../../services/searchCards';
import {
  readSearchColumnShares,
  resizeSearchColumns,
  searchTableLayouts,
  writeSearchColumnShares,
  type SearchTableKind,
} from '../../services/searchTableColumns';
import { rowWindow } from '../../services/virtualWindow';
import SearchPreviewCarousel from './SearchPreviewCarousel.vue';
const props = defineProps<{
  items: SearchItem[];
  scroller?: HTMLElement;
  virtual?: boolean;
  preview?: boolean;
  compactPreview?: boolean;
  carousel?: { label: string; compact: boolean; position: number };
  table?: SearchTableKind;
}>();
const emit = defineEmits<{ position: [index: number] }>();
const TABLE_ROW_HEIGHT = 64;
const layout = computed(() => (props.table ? searchTableLayouts[props.table] : undefined));
const positionedRows = computed(() => !!layout.value && props.virtual);
const grid = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const tableWidth = ref(0);
const shares = shallowRef<readonly number[]>([]);
watch(
  () => props.table,
  (kind) => {
    shares.value = kind ? readSearchColumnShares(kind) : [];
  },
  { immediate: true },
);
const surplus = computed(() =>
  layout.value ? Math.max(0, tableWidth.value - layout.value.baseWidth) : 0,
);
const tracks = computed(() => {
  const config = layout.value;
  return config?.columns
    .map(({ width, share }) =>
      share === undefined
        ? `${width}px`
        : `calc(${width}px + (100% - ${config.baseWidth}px) * ${shares.value[share]})`,
    )
    .join(' ');
});
let drag:
  | {
      index: number;
      pointer: number;
      last: number;
      pending: number;
      before: readonly number[];
    }
  | undefined;
let dragFrame = 0;
function columnWidth(base: number, index: number, max = false) {
  const share = max
    ? shares.value.slice(index).reduce((sum, value) => sum + value, 0)
    : (shares.value[index] ?? 0);
  return Math.round(base + share * surplus.value);
}
function resizeColumn(index: number, pixels: number) {
  shares.value = resizeSearchColumns(shares.value, index, pixels, surplus.value);
}
function applyDrag() {
  if (!drag) return;
  resizeColumn(drag.index, drag.pending - drag.last);
  drag.last = drag.pending;
}
function beginResize(event: PointerEvent, index: number) {
  if (event.button !== 0 || !event.isPrimary || drag || !surplus.value) return;
  const handle = event.currentTarget as HTMLElement;
  handle.focus({ preventScroll: true });
  handle.setPointerCapture(event.pointerId);
  drag = {
    index,
    pointer: event.pointerId,
    last: event.clientX,
    pending: event.clientX,
    before: shares.value,
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
    if (props.table && shares.value !== drag.before)
      writeSearchColumnShares(props.table, shares.value);
  } else {
    shares.value = drag.before;
  }
  drag = undefined;
}
function keyResize(event: KeyboardEvent, index: number) {
  const delta =
    event.key === 'ArrowLeft'
      ? -12
      : event.key === 'ArrowRight'
        ? 12
        : event.key === 'Home'
          ? -surplus.value
          : event.key === 'End'
            ? surplus.value
            : 0;
  if (!delta || drag) return;
  event.preventDefault();
  const before = shares.value;
  resizeColumn(index, delta);
  if (props.table && shares.value !== before) writeSearchColumnShares(props.table, shares.value);
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
  columns.value = props.table
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
  const card = grid.value.querySelector<HTMLElement>('.search-result-card, .guest-search-result');
  if (props.table) pitch.value = TABLE_ROW_HEIGHT;
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
      props.compactPreview,
      props.table,
    ] as const,
  () => {
    scroller?.removeEventListener('scroll', scroll);
    observer?.disconnect();
    scroller = props.scroller;
    tableWidth.value = heading.value?.clientWidth ?? 0;
    if (!grid.value || (!props.virtual && !props.preview && !props.table)) return;
    if (props.virtual) scroller?.addEventListener('scroll', scroll, { passive: true });
    observer = new ResizeObserver(() => {
      tableWidth.value = heading.value?.clientWidth ?? 0;
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
    primary && props.table
      ? [...(buttons ?? [])].find((button) =>
          button.matches(props.table === 'voice' ? '.search-card-play' : '.search-card-open'),
        )
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
  } else if (
    event.target.matches(
      '.search-card-open, :is(.search-voice-card,.search-voice-row) .search-card-play',
    )
  ) {
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
    :class="table ? ['search-collection-table', `search-${table}-table`] : undefined"
    :style="layout ? { '--search-collection-tracks': tracks } : undefined"
    :role="layout ? 'table' : undefined"
    :aria-label="layout?.name"
    :aria-rowcount="layout ? items.length + 1 : undefined"
  >
    <div
      v-if="layout"
      ref="heading"
      :class="['search-collection-columns', `search-${table}-columns`]"
      role="row"
      aria-rowindex="1"
    >
      <span
        v-for="column in layout.columns"
        :key="column.id"
        role="columnheader"
        :aria-label="column.label"
      >
        <span>{{ column.name }}</span>
        <span
          v-if="column.share !== undefined && column.share < shares.length - 1 && surplus > 0"
          :class="['search-collection-column-resizer', `search-${table}-column-resizer`]"
          role="separator"
          aria-orientation="vertical"
          :aria-label="`调整${column.name}列宽`"
          :aria-valuemin="column.width"
          :aria-valuemax="columnWidth(column.width, column.share, true)"
          :aria-valuenow="columnWidth(column.width, column.share)"
          tabindex="0"
          @pointerdown.prevent="beginResize($event, column.share)"
          @pointermove="moveResize"
          @pointerup="finishResize"
          @pointercancel="finishResize"
          @lostpointercapture="finishResize"
          @keydown="keyResize($event, column.share)"
        />
      </span>
    </div>
    <div
      ref="grid"
      class="search-result-grid"
      :class="{
        'search-collection-grid': layout,
        'search-collection-virtual': positionedRows,
        'search-preview-compact': preview && compactPreview,
        'search-person-grid': items.every((i) => i.kind === 'artist' || i.kind === 'user'),
        'search-related-grid': items.every((i) => i.kind === 'related'),
        'search-video-grid': items.every((i) => i.kind === 'mv'),
        'search-voice-grid': !layout && items.every((i) => i.kind === 'voice'),
      }"
      :role="layout ? 'rowgroup' : undefined"
      :style="positionedRows ? { height: `${items.length * TABLE_ROW_HEIGHT}px` } : undefined"
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
        :role="layout ? 'presentation' : undefined"
        :data-grid-index="start + i"
        :style="positionedRows ? { top: `${(start + i) * TABLE_ROW_HEIGHT}px` } : undefined"
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
