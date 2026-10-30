import { computed, onUnmounted, ref, shallowRef, watch } from 'vue';
import { rowWindow } from '../services/virtualWindow';

/** A bounded list inside a shared native scroller, without nested scroll areas. */
export function useOuterVirtualRows(
  count: () => number,
  rowHeight: number,
  root: () => HTMLElement | undefined,
) {
  const container = ref<HTMLElement>();
  const range = shallowRef({ start: 0, end: 0 });
  let frame: number | undefined;
  let stop: (() => void) | undefined;
  function update() {
    frame = undefined;
    const list = container.value,
      outer = root();
    if (!list || !outer) return;
    const a = list.getBoundingClientRect(),
      b = outer.getBoundingClientRect();
    const rowTop = a.top + (parseFloat(getComputedStyle(list).paddingTop) || 0);
    const offset = Math.max(0, b.top - rowTop);
    const visible = Math.max(0, Math.min(count() * rowHeight, b.bottom - rowTop) - offset);
    const next =
      visible > 0
        ? rowWindow(count(), rowHeight, visible, offset, range.value, 4)
        : a.top >= b.bottom
          ? { start: 0, end: 0 }
          : { start: count(), end: count() };
    const focused = document.activeElement as HTMLElement | null;
    if (focused && list.contains(focused) && focused !== list) {
      const index = Number(
        focused.closest<HTMLElement>('[data-sidebar-index]')?.dataset.sidebarIndex,
      );
      if (index < next.start || index >= next.end) list.focus({ preventScroll: true });
    }
    if (next.start !== range.value.start || next.end !== range.value.end) range.value = next;
  }
  function schedule() {
    frame ??= requestAnimationFrame(update);
  }
  watch(
    [container, root],
    ([list, outer]) => {
      stop?.();
      if (!list || !outer) return;
      const observer = new ResizeObserver(schedule);
      observer.observe(outer);
      observer.observe(list);
      if (list.parentElement?.parentElement) observer.observe(list.parentElement.parentElement);
      outer.addEventListener('scroll', schedule, { passive: true });
      stop = () => {
        observer.disconnect();
        outer.removeEventListener('scroll', schedule);
      };
      update();
    },
    { flush: 'post' },
  );
  watch(count, schedule, { flush: 'post' });
  onUnmounted(() => {
    stop?.();
    if (frame !== undefined) cancelAnimationFrame(frame);
  });
  return {
    container,
    start: computed(() => range.value.start),
    end: computed(() => range.value.end),
    update,
  };
}
