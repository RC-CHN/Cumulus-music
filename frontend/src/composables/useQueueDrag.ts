import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { QueueView } from '../services/player';
import { reorderQueueRows } from '../services/queueSelection';

/** In-window pointer sorting avoids OS file drops and retains capture on the
 * scroll container, whose lifetime does not depend on virtual row recycling. */
export function useQueueDrag(options: {
  queue(): QueueView | null | undefined;
  container(): HTMLElement | undefined;
  disabled(): boolean;
  selected(): ReadonlySet<string>;
  select(id: string, toggle: boolean, range: boolean): void;
  scroll(): void;
  reorder(ids: string[], displayed: QueueView, focusId: string): void;
}) {
  const dragging = shallowRef<ReadonlySet<string>>(new Set()),
    boundary = ref<number>();
  let cleanup: (() => void) | undefined;
  const marker = computed(() => (boundary.value === undefined ? undefined : boundary.value * 64));
  function start(event: PointerEvent, id: string) {
    if (event.button !== 0 || options.disabled() || event.pointerType === 'touch') return;
    cleanup?.();
    const displayed = options.queue(),
      element = options.container();
    if (!displayed || !element) return;
    const toggle = event.ctrlKey || event.metaKey,
      range = event.shiftKey;
    const collapse = !toggle && !range && options.selected().has(id);
    if (!collapse) options.select(id, toggle, range);
    const picked = new Set(options.selected());
    const ids = displayed.tracks.map((item) => item.entryId);
    let active = false,
      frame: number | undefined,
      lastTime = 0;
    let focusOwner: Element | null = null;
    let x = event.clientX,
      y = event.clientY;
    const owns = () =>
      !options.disabled() &&
      options.queue()?.epoch === displayed.epoch &&
      options.queue()?.revision === displayed.revision &&
      element === options.container();
    const update = () => {
      const box = element.getBoundingClientRect();
      boundary.value =
        x >= box.left && x <= box.right && y >= box.top && y <= box.bottom
          ? Math.max(
              0,
              Math.min(ids.length, Math.floor((y - box.top + element.scrollTop + 32) / 64)),
            )
          : undefined;
    };
    const scroll = (now: number) => {
      frame = undefined;
      if (!active || !owns()) {
        stop();
        return;
      }
      const box = element.getBoundingClientRect();
      const distance =
        Math.max(0, Math.min(48, y - (box.bottom - 48))) -
        Math.max(0, Math.min(48, box.top + 48 - y));
      if (x >= box.left && x <= box.right && distance) {
        element.scrollTop += (distance * Math.min(40, now - (lastTime || now))) / 80;
        options.scroll();
      }
      lastTime = now;
      update();
      frame = requestAnimationFrame(scroll);
    };
    function stop() {
      if (frame !== undefined) cancelAnimationFrame(frame);
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', end);
      document.removeEventListener('pointercancel', cancel);
      document.removeEventListener('keydown', escape, true);
      element!.removeEventListener('lostpointercapture', cancel);
      window.removeEventListener('blur', stop);
      cleanup = undefined;
      dragging.value = new Set();
      boundary.value = undefined;
      if (element!.hasPointerCapture(event.pointerId))
        element!.releasePointerCapture(event.pointerId);
      if (active && owns() && document.activeElement === element && focusOwner) {
        [...element!.querySelectorAll<HTMLButtonElement>('.queue-song')]
          .find((node) => node.closest<HTMLElement>('.queue-row')?.dataset.entryId === id)
          ?.focus({ preventScroll: true });
      }
    }
    const cancel = (e: PointerEvent) => {
      if (e.pointerId === event.pointerId) stop();
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopImmediatePropagation();
      stop();
    };
    const move = (e: PointerEvent) => {
      if (e.pointerId !== event.pointerId) return;
      if (!owns()) {
        stop();
        return;
      }
      x = e.clientX;
      y = e.clientY;
      if (!active && picked.size && Math.hypot(x - event.clientX, y - event.clientY) >= 6) {
        active = true;
        focusOwner = document.activeElement;
        // The source row can leave the virtual window during edge scrolling.
        // Keep focus on a stable live element until the transaction ends.
        if (element.contains(focusOwner)) element.focus({ preventScroll: true });
        dragging.value = picked;
        try {
          element.setPointerCapture(event.pointerId);
        } catch {
          /* synthetic probe */
        }
        frame = requestAnimationFrame(scroll);
      }
      if (active) {
        e.preventDefault();
        update();
      }
    };
    const end = (e: PointerEvent) => {
      if (e.pointerId !== event.pointerId) return;
      const valid = owns();
      x = e.clientX;
      y = e.clientY;
      if (active) update();
      const next =
        active && valid && boundary.value !== undefined
          ? reorderQueueRows(ids, picked, boundary.value)
          : undefined;
      if (
        !active &&
        valid &&
        collapse &&
        (e.target as HTMLElement).closest('.queue-row')?.getAttribute('data-entry-id') === id
      )
        options.select(id, false, false);
      stop();
      if (next) options.reorder(next, displayed, id);
    };
    cleanup = stop;
    document.addEventListener('pointermove', move, { passive: false });
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', cancel);
    document.addEventListener('keydown', escape, true);
    element.addEventListener('lostpointercapture', cancel);
    window.addEventListener('blur', stop);
  }
  watch([() => options.queue()?.epoch, () => options.queue()?.revision, options.disabled], () =>
    cleanup?.(),
  );
  onBeforeUnmount(() => cleanup?.());
  return { dragging, boundary, marker, start };
}
