import { computed, onBeforeUnmount, ref, shallowRef, watch, type Ref } from 'vue';
import {
  batchColumnWidths,
  batchDefaultGrowth,
  batchMinimumWidth,
  resizeBatchColumns,
  type BatchGrowth,
  type BatchResizeColumn,
} from '../services/playlistBatchTable';

export function usePlaylistBatchColumns(
  container: Ref<HTMLElement | undefined>,
  disabled: () => boolean,
) {
  const width = ref(batchMinimumWidth),
    growth = shallowRef<BatchGrowth>([...batchDefaultGrowth]);
  const dragging = ref<BatchResizeColumn>();
  let observer: ResizeObserver | undefined, cleanup: (() => void) | undefined;
  const widths = computed(() => batchColumnWidths(width.value, growth.value));
  const style = computed(() => ({
    gridTemplateColumns: widths.value.map((n) => `${n}px`).join(' '),
  }));
  const minimum = (column: BatchResizeColumn) => (column === 0 ? 180 : 115);
  const value = (column: BatchResizeColumn) => widths.value[column + 1]!;
  const maximum = (column: BatchResizeColumn) =>
    value(column) + (widths.value[4]! - 64) + (column === 0 ? widths.value[2]! - 115 : 0);
  const available = computed(() => width.value > batchMinimumWidth + 1 && !disabled());
  watch(
    container,
    (element) => {
      observer?.disconnect();
      if (!element) return;
      const measure = () => {
        const measured = Math.max(batchMinimumWidth, element.clientWidth);
        if (measured === width.value) return;
        // Cancel a pointer transaction before applying a different width basis.
        cleanup?.();
        width.value = measured;
      };
      measure();
      observer = new ResizeObserver(measure);
      observer.observe(element);
    },
    { flush: 'post' },
  );
  function reset() {
    if (disabled()) return;
    cleanup?.();
    growth.value = [...batchDefaultGrowth];
  }
  function key(event: KeyboardEvent, column: BatchResizeColumn) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End', 'Enter', ' '].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    if (!available.value) return;
    if (event.key === 'Enter' || event.key === ' ') {
      reset();
      return;
    }
    const delta =
      event.key === 'Home'
        ? minimum(column) - value(column)
        : event.key === 'End'
          ? maximum(column) - value(column)
          : (event.key === 'ArrowRight' ? 1 : -1) * (event.shiftKey ? 32 : 8);
    growth.value = resizeBatchColumns(growth.value, column, delta, width.value);
  }
  function start(event: PointerEvent, column: BatchResizeColumn) {
    if (!available.value || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    cleanup?.();
    const handle = event.currentTarget as HTMLElement;
    handle.focus({ preventScroll: true });
    const original: BatchGrowth = [...growth.value],
      pointer = event.pointerId;
    const basis = width.value;
    let last = event.clientX,
      frame: number | undefined,
      pending = last;
    dragging.value = column;
    const apply = () => {
      frame = undefined;
      growth.value = resizeBatchColumns(growth.value, column, pending - last, basis);
      last = pending;
    };
    const move = (e: PointerEvent) => {
      if (e.pointerId !== pointer) return;
      e.preventDefault();
      pending = e.clientX;
      frame ??= requestAnimationFrame(apply);
    };
    const stop = () => {
      if (frame !== undefined) {
        cancelAnimationFrame(frame);
        apply();
      }
      dragging.value = undefined;
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', end);
      document.removeEventListener('pointercancel', pointerCancel);
      handle.removeEventListener('lostpointercapture', pointerCancel);
      document.removeEventListener('keydown', escape, true);
      window.removeEventListener('blur', cancel);
      cleanup = undefined;
      if (handle.hasPointerCapture(pointer)) handle.releasePointerCapture(pointer);
    };
    const cancel = () => {
      stop();
      growth.value = original;
    };
    const pointerCancel = (e: PointerEvent) => {
      if (e.pointerId === pointer) cancel();
    };
    const end = (e: PointerEvent) => {
      if (e.pointerId === pointer) {
        if (frame !== undefined) cancelAnimationFrame(frame);
        pending = e.clientX;
        apply();
        stop();
      }
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        cancel();
      }
    };
    cleanup = cancel;
    document.addEventListener('pointermove', move, { passive: false });
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', pointerCancel);
    handle.addEventListener('lostpointercapture', pointerCancel);
    document.addEventListener('keydown', escape, true);
    window.addEventListener('blur', cancel);
    // Synthetic WebView probes have no active OS pointer; document listeners
    // exercise the same bounded transaction in that case.
    try {
      handle.setPointerCapture(pointer);
    } catch {
      /* no active pointer */
    }
  }
  watch(disabled, (value) => {
    if (value) cleanup?.();
  });
  onBeforeUnmount(() => {
    cleanup?.();
    observer?.disconnect();
  });
  return { style, widths, minimum, maximum, value, available, dragging, key, start, reset };
}
