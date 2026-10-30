import { nextTick, ref, watch, type Ref } from 'vue';
import { batchSelectionNeighbor } from '../services/playlistBatchTable';
import { useVirtualRows } from './useVirtualRows';

/** Shared selection, keyboard focus and bounded 64px rows for batch tables. */
export function useBatchRows(
  ids: Readonly<Ref<string[]>>,
  options: {
    selected(): Set<string>;
    disabled(): boolean;
    toggle(index: number, range: boolean): void;
    select(index: number): void;
    all(force: boolean): void;
  },
) {
  const { container, start, end, onScroll } = useVirtualRows(() => ids.value.length, 64);
  const active = ref(0);
  let recycling = false;
  watch(
    [start, end],
    ([from, to]) => {
      const row = document.activeElement?.closest<HTMLElement>('[data-batch-index]');
      if (!row || !container.value?.contains(row)) return;
      const index = Number(row.dataset.batchIndex);
      if (index < from || index >= to) {
        active.value = Math.min(ids.value.length - 1, Math.ceil(container.value.scrollTop / 64));
        recycling = true;
        container.value.focus({ preventScroll: true });
        recycling = false;
      }
    },
    { flush: 'pre' },
  );
  function toggle(event: MouseEvent, index: number) {
    if (options.disabled()) return;
    (event.currentTarget as HTMLElement)
      .closest<HTMLElement>('[data-batch-index]')
      ?.focus({ preventScroll: true });
    options.toggle(index, event.shiftKey);
  }
  async function key(event: KeyboardEvent, index: number) {
    if (options.disabled() || event.altKey) return;
    if (event.key === ' ' || event.key === 'Enter') {
      event.preventDefault();
      event.stopPropagation();
      options.toggle(index, event.shiftKey);
      return;
    }
    let target = index;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const direction = event.key === 'ArrowDown' ? 1 : -1;
      if (!event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
        const neighbor = batchSelectionNeighbor(ids.value, options.selected(), direction);
        event.preventDefault();
        event.stopPropagation();
        if (neighbor === undefined) return;
        target = neighbor;
      } else target += direction;
    } else if (event.key === 'Home') target = 0;
    else if (event.key === 'End') target = ids.value.length - 1;
    else return;
    event.preventDefault();
    event.stopPropagation();
    target = Math.max(0, Math.min(ids.value.length - 1, target));
    if (event.shiftKey) options.toggle(target, true);
    else if (!event.ctrlKey && !event.metaKey) options.select(target);
    await focusIndex(target);
  }
  async function focusBody(event: FocusEvent) {
    if (recycling || options.disabled() || !event.relatedTarget) return;
    const selected = ids.value.findIndex((id) => options.selected().has(id));
    if (selected < 0) options.select(0);
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
      if (!options.disabled()) options.all(true);
    }
  }
  return { container, start, end, onScroll, active, toggle, key, focusBody, shortcut };
}
