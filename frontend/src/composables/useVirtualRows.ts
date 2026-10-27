import { computed, onUnmounted, ref, shallowRef, watch } from 'vue';
import { rowWindow } from '../services/virtualWindow';

/** Native passive scrolling; Vue only receives changes to the buffered row range. */
export function useVirtualRows(count: () => number, rowHeight: number) {
  const container = ref<HTMLElement>();
  const range = shallowRef(rowWindow(count(), rowHeight, 500, 0));
  let height = 500;
  let frame: number | undefined;
  let observer: ResizeObserver | undefined;
  function update() {
    frame = undefined;
    range.value = rowWindow(
      count(),
      rowHeight,
      height,
      container.value?.scrollTop ?? 0,
      range.value,
    );
  }
  function onScroll() {
    frame ??= requestAnimationFrame(update);
  }
  function reset() {
    if (container.value) container.value.scrollTop = 0;
    range.value = rowWindow(count(), rowHeight, height, 0);
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
    count,
    () => {
      const element = container.value;
      if (element)
        element.scrollTop = Math.min(element.scrollTop, Math.max(0, count() * rowHeight - height));
      // Shrinking lists must not retain an out-of-bounds buffer.
      range.value = rowWindow(count(), rowHeight, height, element?.scrollTop ?? 0);
    },
    { flush: 'post' },
  );
  onUnmounted(() => {
    observer?.disconnect();
    if (frame !== undefined) cancelAnimationFrame(frame);
  });
  return {
    container,
    start: computed(() => range.value.start),
    end: computed(() => range.value.end),
    onScroll,
    reset,
  };
}
