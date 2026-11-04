import { computed, onUnmounted, ref, shallowRef, watch } from 'vue';
import { rowWindow } from '../services/virtualWindow';

export const PLAYLIST_END_THRESHOLD = 60;

/** Shared scroll surface, bounded fixed-height window, and official near-end trigger. */
export function usePlaylistWaterfall(
  scroller: () => HTMLElement | undefined,
  rows: (width: number) => number,
  height: (width: number) => number,
  canLoad: () => boolean,
  load: () => void,
) {
  const list = ref<HTMLElement>();
  const width = ref(0);
  const count = computed(() => rows(width.value));
  const rowHeight = computed(() => height(width.value));
  const total = computed(() => count.value * rowHeight.value);
  const range = shallowRef({ start: 0, end: 0 });
  let frame: number | undefined;
  let dispose: (() => void) | undefined;
  function update() {
    const outer = scroller(),
      grid = list.value;
    if (!outer || !grid || outer.clientHeight <= 0) return;
    width.value = grid.clientWidth;
    const top = grid.getBoundingClientRect().top - outer.getBoundingClientRect().top;
    const offset = Math.max(0, -top);
    const viewport = Math.max(0, outer.clientHeight - Math.max(0, top));
    const next = rowWindow(count.value, rowHeight.value, viewport, offset, range.value, 4);
    const active = document.activeElement;
    if (active instanceof HTMLElement && grid.contains(active)) {
      const index = Number(
        active.closest<HTMLElement>('[data-playlist-window-row]')?.dataset.playlistWindowRow,
      );
      if (Number.isFinite(index) && (index < next.start || index >= next.end))
        grid.focus({ preventScroll: true });
    }
    if (next.start !== range.value.start || next.end !== range.value.end) range.value = next;
  }
  function tick() {
    frame = undefined;
    update();
    const outer = scroller();
    if (!outer || outer.clientHeight <= 0 || outer.closest('[inert]') || !canLoad()) return;
    const style = getComputedStyle(outer);
    const padding = (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
    if (
      outer.scrollTop + outer.clientHeight + padding + PLAYLIST_END_THRESHOLD >=
      outer.scrollHeight
    )
      load();
  }
  function schedule() {
    frame ??= requestAnimationFrame(tick);
  }
  watch(
    [list, scroller],
    ([grid, outer]) => {
      dispose?.();
      if (!grid || !outer) return;
      const observer = new ResizeObserver(schedule);
      observer.observe(grid);
      observer.observe(outer);
      outer.addEventListener('scroll', schedule, { passive: true });
      dispose = () => {
        observer.disconnect();
        outer.removeEventListener('scroll', schedule);
      };
      update();
      schedule();
    },
    { flush: 'post' },
  );
  watch([count, rowHeight, canLoad], schedule, { flush: 'post' });
  onUnmounted(() => {
    dispose?.();
    if (frame !== undefined) cancelAnimationFrame(frame);
  });
  return { list, width, total, rowHeight, range, update };
}
