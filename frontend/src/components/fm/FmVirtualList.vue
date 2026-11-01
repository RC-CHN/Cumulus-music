<script setup lang="ts" generic="T extends { id: string }">
import { nextTick, watch } from 'vue';
import { useOuterVirtualRows } from '../../composables/useOuterVirtualRows';
const props = defineProps<{ items: T[]; rowHeight: number; label: string }>();
const { container, start, end, update } = useOuterVirtualRows(
  () => props.items.length,
  props.rowHeight,
  (list) => list?.closest<HTMLElement>('.main-content') ?? undefined,
  (element) => Number(element.closest<HTMLElement>('[data-fm-index]')?.dataset.fmIndex),
);
let request = 0;
watch(
  () => props.items,
  () => request++,
);
async function keyboard(event: KeyboardEvent) {
  if (
    event.altKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.shiftKey ||
    !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)
  )
    return;
  const list = container.value,
    root = list?.closest<HTMLElement>('.main-content');
  if (!list || !root || !props.items.length) return;
  event.preventDefault();
  event.stopPropagation();
  const trigger = event.target as HTMLElement;
  const current = trigger.closest<HTMLElement>('[data-fm-index]');
  const visible = Math.max(
    0,
    Math.floor(
      (root.getBoundingClientRect().top - list.getBoundingClientRect().top) / props.rowHeight,
    ),
  );
  const from = current ? Number(current.dataset.fmIndex) : visible;
  const index = Math.max(
    0,
    Math.min(
      props.items.length - 1,
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? props.items.length - 1
          : from + (event.key === 'ArrowDown' ? 1 : -1),
    ),
  );
  const id = props.items[index].id,
    attempt = ++request;
  const top =
    list.getBoundingClientRect().top -
    root.getBoundingClientRect().top +
    root.scrollTop +
    index * props.rowHeight;
  if (top < root.scrollTop) root.scrollTop = top;
  else if (top + props.rowHeight > root.scrollTop + root.clientHeight)
    root.scrollTop = top + props.rowHeight - root.clientHeight;
  update();
  await nextTick();
  if (
    request !== attempt ||
    !list.isConnected ||
    props.items[index]?.id !== id ||
    ![trigger, list, document.body].includes(document.activeElement as HTMLElement)
  )
    return;
  const row = list.querySelector<HTMLElement>(`[data-fm-index="${index}"]`);
  const selector = trigger.matches('button')
    ? 'button:not(:disabled)'
    : 'input:not(:disabled), button:not(:disabled)';
  (row?.querySelector<HTMLElement>(selector) ?? row)?.focus({ preventScroll: true });
}
</script>
<template>
  <div
    ref="container"
    class="fm-virtual-list"
    role="list"
    :aria-label="label"
    tabindex="0"
    :data-total="items.length"
    @keydown="keyboard"
  >
    <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
    <div
      v-for="(item, offset) in items.slice(start, end)"
      :key="item.id"
      class="fm-virtual-row"
      role="listitem"
      tabindex="-1"
      :data-fm-index="start + offset"
      :aria-posinset="start + offset + 1"
      :aria-setsize="items.length"
      :style="{ height: `${rowHeight}px` }"
    >
      <slot :item="item" :index="start + offset" />
    </div>
    <div :style="{ height: `${(items.length - end) * rowHeight}px` }" aria-hidden="true" />
  </div>
</template>
<style scoped>
.fm-virtual-list {
  min-width: 0;
}
.fm-virtual-row {
  box-sizing: border-box;
  min-width: 0;
}
.fm-virtual-list:focus-visible,
.fm-virtual-row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  border-radius: 6px;
}
</style>
