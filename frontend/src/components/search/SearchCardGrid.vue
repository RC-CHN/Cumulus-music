<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { SearchItem } from '../../services/search';
import { searchCardKey } from '../../services/searchCards';
import { rowWindow } from '../../services/virtualWindow';
const props = defineProps<{
  items: SearchItem[];
  scroller?: HTMLElement;
  virtual?: boolean;
  preview?: boolean;
}>();
const grid = ref<HTMLElement>();
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
  props.virtual && range.value.start ? range.value.start * pitch.value - 20 : 0,
);
const bottom = computed(() =>
  props.virtual && range.value.end < count.value
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
  columns.value = Math.max(1, getComputedStyle(grid.value).gridTemplateColumns.split(' ').length);
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
  if (card) pitch.value = card.getBoundingClientRect().height + 20;
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
  () => [grid.value, props.scroller, props.virtual, props.preview] as const,
  () => {
    scroller?.removeEventListener('scroll', scroll);
    observer?.disconnect();
    scroller = props.scroller;
    if (!grid.value || !scroller || (!props.virtual && !props.preview)) return;
    if (props.virtual) scroller.addEventListener('scroll', scroll, { passive: true });
    observer = new ResizeObserver(() => update(true));
    observer.observe(grid.value);
    observer.observe(scroller);
    update();
  },
  { flush: 'post' },
);
watch(
  () => props.items.length,
  () => update(),
  { flush: 'post' },
);
async function focusItem(index: number, last = false) {
  if (!scroller) return;
  const row = Math.floor(index / columns.value),
    target = gridOffset() + row * pitch.value;
  if (target < scroller.scrollTop) scroller.scrollTop = target;
  else if (target + pitch.value > scroller.scrollTop + scroller.clientHeight)
    scroller.scrollTop = target + pitch.value - scroller.clientHeight;
  update();
  await nextTick();
  const buttons = grid.value
    ?.querySelector(`[data-grid-index="${index}"]`)
    ?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
  (last ? buttons?.[buttons.length - 1] : buttons?.[0])?.focus({ preventScroll: true });
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
  } else if (event.target.classList.contains('search-card-open')) {
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
      void focusItem(Math.max(0, Math.min(props.items.length - 1, move[event.key]!)));
    }
  }
}
onBeforeUnmount(() => {
  scroller?.removeEventListener('scroll', scroll);
  observer?.disconnect();
  cancelAnimationFrame(frame);
});
</script>
<template>
  <div
    ref="grid"
    class="search-result-grid"
    :class="{
      'search-person-grid': items.every((i) => i.kind === 'artist' || i.kind === 'user'),
      'search-related-grid': items.every((i) => i.kind === 'related'),
      'search-video-grid': items.every((i) => i.kind === 'mv'),
      'search-voice-grid': items.every((i) => i.kind === 'voice'),
    }"
    :data-loaded-count="items.length"
    :data-preview="preview || undefined"
    @keydown="keydown"
  >
    <div v-if="top" aria-hidden="true" class="search-grid-spacer" :style="{ height: `${top}px` }" />
    <div
      v-for="(item, i) in visible"
      :key="searchCardKey(item)"
      class="search-grid-cell"
      :data-grid-index="start + i"
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
</template>
