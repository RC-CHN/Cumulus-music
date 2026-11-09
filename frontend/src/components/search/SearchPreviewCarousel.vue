<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { SearchItem } from '../../services/search';
import { searchCardKey } from '../../services/searchCards';
import Icon from '../Icon.vue';

const props = defineProps<{
  items: SearchItem[];
  label: string;
  compact: boolean;
  position: number;
}>();
const emit = defineEmits<{ position: [index: number] }>();
const viewport = ref<HTMLElement>();
const width = ref(0);
const moving = ref(false);
const gap = computed(() => (props.compact ? 16 : 20));
const columns = computed(() => {
  const video = props.items.every((item) => item.kind === 'mv');
  const span = props.compact && !video ? 2 : 3;
  return Math.max(1, Math.min(Math.floor(18 / span), Math.floor((width.value + 20) / (span * 66))));
});
const pageCount = computed(() => Math.max(1, Math.ceil(props.items.length / columns.value)));
const lastStart = computed(() => Math.max(0, props.items.length - columns.value));
const page = computed(() =>
  props.position >= lastStart.value
    ? pageCount.value - 1
    : Math.floor(Math.max(0, props.position) / columns.value),
);
const start = computed(() => Math.min(lastStart.value, page.value * columns.value));
const unit = computed(() => (width.value + gap.value) / columns.value);
const visible = (index: number) => index >= start.value && index < start.value + columns.value;
let observer: ResizeObserver | undefined;

function move(step: -1 | 1) {
  const target = page.value + step;
  if (target < 0 || target >= pageCount.value) return;
  moving.value = true;
  emit('position', Math.min(lastStart.value, target * columns.value));
}

function measure() {
  const node = viewport.value;
  if (!node) return;
  const next = node.clientWidth;
  if (!next || next === width.value) return;
  const active = document.activeElement;
  const cell =
    active instanceof HTMLElement ? active.closest<HTMLElement>('[data-grid-index]') : null;
  const index = cell && node.contains(cell) ? Number(cell.dataset.gridIndex) : undefined;
  moving.value = false;
  width.value = next;
  if (index !== undefined && !visible(index)) {
    void nextTick(() => {
      if (document.activeElement !== document.body && document.activeElement !== active) return;
      node
        .querySelector<HTMLButtonElement>(
          '[data-grid-index]:not([inert]) .search-card-open:not(:disabled)',
        )
        ?.focus({ preventScroll: true });
    });
  }
}

watch(
  viewport,
  (node) => {
    observer?.disconnect();
    if (!node) return;
    observer = new ResizeObserver(measure);
    observer.observe(node);
    measure();
  },
  { flush: 'post' },
);
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div
    class="search-carousel"
    :class="{ 'search-preview-compact': compact }"
    role="group"
    :aria-label="`${label}预览`"
    :data-preview-start="start"
    :data-preview-columns="columns"
  >
    <div ref="viewport" class="search-carousel-viewport">
      <div
        class="search-carousel-rail"
        :class="{ 'search-carousel-moving': moving }"
        :style="{
          gap: `${gap}px`,
          gridAutoColumns: `${Math.max(0, unit - gap)}px`,
          transform: `translateX(${-start * unit}px)`,
        }"
      >
        <div
          v-for="(item, index) in items"
          :key="`${index}:${searchCardKey(item)}`"
          class="search-grid-cell"
          :data-grid-index="index"
          :inert="!visible(index)"
        >
          <slot :item="item" :index="index" />
        </div>
      </div>
    </div>
    <template v-if="pageCount > 1">
      <button
        type="button"
        class="search-carousel-arrow previous"
        :aria-label="`${label}预览上一页`"
        :aria-disabled="page === 0"
        @click="move(-1)"
      >
        <Icon name="back" />
      </button>
      <button
        type="button"
        class="search-carousel-arrow next"
        :aria-label="`${label}预览下一页`"
        :aria-disabled="page + 1 === pageCount"
        @click="move(1)"
      >
        <Icon name="forward" />
      </button>
      <span class="sr-only" role="status">第 {{ page + 1 }} 页，共 {{ pageCount }} 页</span>
    </template>
  </div>
</template>

<style scoped>
.search-carousel {
  position: relative;
  min-width: 0;
}
.search-carousel-viewport {
  overflow: clip;
}
.search-carousel-rail {
  display: grid;
  grid-auto-flow: column;
}
.search-carousel-moving {
  transition: transform 0.6s ease;
}
.search-carousel-arrow {
  position: absolute;
  top: 45%;
  display: grid;
  place-items: center;
  width: 28px;
  height: 34px;
  padding: 0;
  transform: translateY(-50%);
  border: 1px solid var(--line);
  border-radius: 8px;
  color: var(--text);
  background: var(--surface);
  cursor: pointer;
  z-index: 2;
}
.search-carousel-arrow.previous {
  left: 2px;
}
.search-carousel-arrow.next {
  right: 2px;
}
.search-carousel-arrow[aria-disabled='true'] {
  opacity: 0.45;
  cursor: default;
}
@media (prefers-reduced-motion: reduce) {
  .search-carousel-moving {
    transition: none;
  }
}
</style>
