import { computed, nextTick, ref, watch, type Ref } from 'vue';
import type { DownloadTask } from '../services/downloads';

// Native scrolling may unmount a card while a button inside it owns focus.
export function useDownloadMvFocus(
  tasks: () => DownloadTask[],
  grid: Ref<HTMLElement | undefined>,
  columns: Ref<number>,
  height: Ref<number>,
  start: Ref<number>,
  end: Ref<number>,
  update: () => void,
) {
  const active = ref('');
  const indexById = computed(() => new Map(tasks().map((item, i) => [item.id, i])));
  const firstVisible = () =>
    Math.min(
      tasks().length - 1,
      Math.max(0, Math.ceil((grid.value?.scrollTop ?? 0) / height.value) * columns.value),
    );
  const roving = computed(() => {
    const i = indexById.value.get(active.value) ?? -1;
    return i >= start.value * columns.value && i < end.value * columns.value
      ? active.value
      : tasks()[firstVisible()]?.id;
  });
  function focusIn(e: FocusEvent) {
    const card = (e.target as HTMLElement).closest<HTMLElement>('[data-download-mv]');
    if (card?.dataset.downloadMv) active.value = card.dataset.downloadMv;
  }
  watch(
    [start, end, columns, indexById],
    () => {
      const owner = document.activeElement as HTMLElement | null;
      const card = owner?.closest<HTMLElement>('[data-download-mv]');
      if (!card || !grid.value?.contains(card)) return;
      const i = indexById.value.get(card.dataset.downloadMv!) ?? -1;
      const row = Math.floor(i / columns.value);
      if (row < start.value || row >= end.value) {
        active.value = tasks()[firstVisible()]?.id ?? '';
        grid.value.focus({ preventScroll: true });
      } else if (Number(card.parentElement?.dataset.mvRow) !== row) {
        // Reflowing from four to two columns can reparent an otherwise visible card.
        const action = owner?.classList.contains('download-mv-manage')
          ? '.download-mv-manage'
          : '.download-mv-cover';
        grid.value.focus({ preventScroll: true });
        void nextTick(() => {
          if (document.activeElement === grid.value)
            grid.value
              ?.querySelector<HTMLElement>(`[data-mv-index="${i}"] ${action}`)
              ?.focus({ preventScroll: true });
        });
      }
    },
    { flush: 'pre' },
  );
  async function key(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || !grid.value || !tasks().length) return;
    const owner = e.target as HTMLElement;
    if (owner !== grid.value && !owner.closest('[data-download-mv]')) return;
    const i =
      owner === grid.value ? firstVisible() : (indexById.value.get(active.value) ?? firstVisible());
    const offset: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -columns.value,
      ArrowDown: columns.value,
    };
    const target =
      e.key === 'Home'
        ? 0
        : e.key === 'End'
          ? tasks().length - 1
          : e.key in offset
            ? i + offset[e.key]!
            : undefined;
    if (target === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    const j = Math.max(0, Math.min(tasks().length - 1, target)),
      id = tasks()[j]!.id;
    const row = Math.floor(j / columns.value) * height.value;
    const action = owner.classList.contains('download-mv-manage')
      ? '.download-mv-manage'
      : '.download-mv-cover';
    const el = grid.value;
    el.focus({ preventScroll: true });
    active.value = id;
    if (row < el.scrollTop) el.scrollTop = row;
    else if (row + height.value > el.scrollTop + el.clientHeight)
      el.scrollTop = row + height.value - el.clientHeight;
    update();
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    await nextTick();
    if (document.activeElement !== el || active.value !== id || tasks()[j]?.id !== id) return;
    el.querySelector<HTMLElement>(`[data-mv-index="${j}"] ${action}`)?.focus({
      preventScroll: true,
    });
  }
  return { roving, focusIn, key };
}
