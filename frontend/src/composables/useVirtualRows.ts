import { computed, onUnmounted, ref, shallowRef, watch } from 'vue';
import { centeredRowTop, rowTop, rowWindow, type RowLayout } from '../services/virtualWindow';

/** Native passive scrolling; Vue only receives changes to the buffered row range. */
export function useVirtualRows(
  count: () => number,
  rowHeight: number | (() => number),
  buffer = 8,
  expansion: () => RowLayout | undefined = () => undefined,
) {
  const getRowHeight = () => (typeof rowHeight === 'number' ? rowHeight : rowHeight());
  const container = ref<HTMLElement>();
  const range = shallowRef(
    rowWindow(count(), getRowHeight(), 500, 0, undefined, buffer, expansion()),
  );
  let height = 500;
  let frame: number | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  let observer: ResizeObserver | undefined;
  function cancelUpdate() {
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (deadline !== undefined) clearTimeout(deadline);
    frame = undefined;
    deadline = undefined;
  }
  function update() {
    cancelUpdate();
    range.value = rowWindow(
      count(),
      getRowHeight(),
      height,
      container.value?.scrollTop ?? 0,
      range.value,
      buffer,
      expansion(),
    );
  }
  function onScroll() {
    if (frame !== undefined) return;
    frame = requestAnimationFrame(update);
    // Programmatic scroll restoration must also populate rows on WebViews
    // that run timers while their compositor withholds animation frames.
    deadline = setTimeout(update, 64);
  }
  function reset() {
    cancelUpdate();
    if (container.value) container.value.scrollTop = 0;
    range.value = rowWindow(count(), getRowHeight(), height, 0, undefined, buffer, expansion());
  }
  function restore(top: number) {
    const element = container.value;
    if (!element || !Number.isFinite(top)) return;
    element.scrollTop = Math.max(0, top);
    height = element.clientHeight;
    update();
  }
  function center(index: number) {
    const element = container.value;
    if (!element) return false;
    const top = centeredRowTop(index, count(), getRowHeight(), element.clientHeight, expansion());
    if (top === null) return false;
    element.scrollTop = top;
    height = element.clientHeight;
    update();
    return true;
  }
  watch(
    container,
    (element) => {
      observer?.disconnect();
      if (!element) return;
      height = element.clientHeight;
      observer = new ResizeObserver((entries) => {
        height = entries[0]?.contentRect.height ?? height;
        onScroll();
      });
      observer.observe(element);
      update();
    },
    { flush: 'post' },
  );
  watch(
    [count, getRowHeight, expansion],
    () => {
      const element = container.value;
      if (element)
        element.scrollTop = Math.min(
          element.scrollTop,
          Math.max(0, rowTop(count(), getRowHeight(), expansion()) - height),
        );
      // Shrinking lists must not retain an out-of-bounds buffer.
      range.value = rowWindow(
        count(),
        getRowHeight(),
        height,
        element?.scrollTop ?? 0,
        undefined,
        buffer,
        expansion(),
      );
    },
    { flush: 'post' },
  );
  onUnmounted(() => {
    observer?.disconnect();
    cancelUpdate();
  });
  return {
    container,
    start: computed(() => range.value.start),
    end: computed(() => range.value.end),
    onScroll,
    reset,
    restore,
    center,
  };
}
