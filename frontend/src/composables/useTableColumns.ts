import { computed, onBeforeUnmount, ref, shallowRef, watch, type Ref } from 'vue';
import {
  tableColumnWidths,
  minimumTableWidth,
  resizeTableColumns,
  type TableColumn,
} from '../services/tableColumns';

export function useTableColumns(
  container: Ref<HTMLElement | undefined>,
  definition: () => readonly TableColumn[],
  disabled: () => boolean,
  growth: Ref<number[]> = shallowRef(definition().map((column) => column.grow)),
) {
  const minimumWidth = computed(() => minimumTableWidth(definition()));
  const width = ref(minimumWidth.value);
  const dragging = ref<number>();
  let disposed = false;
  let observer: ResizeObserver | undefined, cleanup: (() => void) | undefined;
  const widths = computed(() => tableColumnWidths(width.value, definition(), growth.value));
  const style = computed(() => ({
    gridTemplateColumns: widths.value.map((n) => `${n}px`).join(' '),
  }));
  const minimum = (column: number) => definition()[column]!.width;
  const value = (column: number) => widths.value[column]!;
  const resizable = (column: number) =>
    !!definition()[column]?.resize && definition().some((c, i) => i > column && c.resize);
  const maximum = (column: number) =>
    value(column) +
    definition().reduce(
      (sum, c, i) => sum + (i > column && c.resize ? widths.value[i]! - c.width : 0),
      0,
    );
  const available = computed(() => width.value > minimumWidth.value + 1 && !disabled());
  watch(definition, () => {
    cleanup?.();
    growth.value = definition().map((column) => column.grow);
    width.value = Math.max(minimumWidth.value, container.value?.clientWidth ?? 0);
  });
  watch(
    container,
    (element) => {
      observer?.disconnect();
      if (!element) return;
      const measure = () => {
        const measured = Math.max(minimumWidth.value, element.clientWidth);
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
    growth.value = definition().map((column) => column.grow);
  }
  function key(event: KeyboardEvent, column: number) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End', 'Enter', ' '].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    if (!available.value || !resizable(column)) return;
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
    growth.value = resizeTableColumns(definition(), growth.value, column, delta, width.value);
  }
  function start(event: PointerEvent, column: number) {
    if (!available.value || !resizable(column) || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    cleanup?.();
    const handle = event.currentTarget as HTMLElement;
    handle.focus({ preventScroll: true });
    const original = [...growth.value],
      pointer = event.pointerId;
    const basis = width.value;
    let last = event.clientX,
      frame: number | undefined,
      pending = last;
    dragging.value = column;
    const apply = () => {
      frame = undefined;
      if (disposed) return;
      growth.value = resizeTableColumns(definition(), growth.value, column, pending - last, basis);
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
      if (!disposed) growth.value = original;
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
    // An outgoing table must not write its saved widths into its parent's
    // newly selected category. Only cancel listeners after ownership ends.
    disposed = true;
    cleanup?.();
    observer?.disconnect();
  });
  return {
    style,
    widths,
    minimum,
    maximum,
    value,
    resizable,
    available,
    dragging,
    key,
    start,
    reset,
  };
}
