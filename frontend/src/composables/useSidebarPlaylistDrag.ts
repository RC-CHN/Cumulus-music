import { onBeforeUnmount, ref, watch } from 'vue';
import type { PlaylistView } from '../services/contracts';
import { sidebarMoveOrder, type SidebarPlaylistMove } from '../services/sidebar';

const ROW = 46;
const PADDING = 4;

/** Sort only within the visible group; the source may scroll out of its virtual window. */
export function useSidebarPlaylistDrag(options: {
  group(): SidebarPlaylistMove['group'];
  playlists(): PlaylistView[];
  root(): HTMLElement | undefined;
  list(): HTMLElement | undefined;
  disabled(): boolean;
  commit(move: SidebarPlaylistMove): void;
}) {
  const boundary = ref<number>();
  const dragging = ref<string>();
  let cleanup: (() => void) | undefined;
  let suppressUntil = 0;

  function start(event: PointerEvent, id: string) {
    if (event.button !== 0 || event.pointerType === 'touch' || options.disabled()) return;
    cleanup?.();
    suppressUntil = 0;
    const root = options.root();
    const list = options.list();
    const displayed = options.playlists();
    if (!root || !list || !displayed.some((item) => item.id === id && !item.favorite)) return;
    const group = options.group();
    let x = event.clientX,
      y = event.clientY,
      frame: number | undefined,
      last = 0,
      active = false;
    const owns = () =>
      root === options.root() &&
      list === options.list() &&
      displayed === options.playlists() &&
      !options.disabled();
    function update() {
      const viewport = root!.getBoundingClientRect();
      const box = list!.getBoundingClientRect();
      boundary.value =
        x >= box.left &&
        x <= box.right &&
        y >= Math.max(box.top, viewport.top) &&
        y <= Math.min(box.bottom, viewport.bottom)
          ? Math.max(0, Math.min(displayed.length, Math.round((y - box.top - PADDING) / ROW)))
          : undefined;
      return boundary.value === undefined
        ? 0
        : Math.max(0, y - viewport.bottom + 40) - Math.max(0, viewport.top + 40 - y);
    }
    function scroll(time: number) {
      frame = undefined;
      if (!owns()) return stop();
      const distance = update();
      if (distance) {
        const position = root!.scrollTop;
        const delta = (distance * Math.min(32, time - (last || time - 16))) / 60;
        root!.scrollTop = position + (delta > 0 ? Math.ceil(delta) : Math.floor(delta));
        last = time;
        if (root!.scrollTop !== position) frame = requestAnimationFrame(scroll);
      }
    }
    function moved() {
      if (active && frame === undefined && update()) {
        last = 0;
        frame = requestAnimationFrame(scroll);
      }
    }
    function stop() {
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', end);
      document.removeEventListener('pointercancel', cancel);
      document.removeEventListener('keydown', escape, true);
      window.removeEventListener('blur', stop);
      root!.removeEventListener('scroll', moved);
      list!.removeEventListener('lostpointercapture', cancel);
      if (frame !== undefined) cancelAnimationFrame(frame);
      if (list!.hasPointerCapture(event.pointerId)) list!.releasePointerCapture(event.pointerId);
      if (active) suppressUntil = performance.now() + 220;
      dragging.value = undefined;
      boundary.value = undefined;
      cleanup = undefined;
    }
    function cancel(input: PointerEvent) {
      if (input.pointerId === event.pointerId) stop();
    }
    function escape(input: KeyboardEvent) {
      if (!active || input.key !== 'Escape') return;
      input.preventDefault();
      input.stopImmediatePropagation();
      stop();
    }
    function move(input: PointerEvent) {
      if (input.pointerId !== event.pointerId) return;
      if (!owns()) return stop();
      x = input.clientX;
      y = input.clientY;
      if (!active && Math.hypot(x - event.clientX, y - event.clientY) >= 6) {
        active = true;
        dragging.value = id;
        try {
          list!.setPointerCapture(event.pointerId);
        } catch {
          // Keyboard/probe-created pointer IDs need not have native capture.
        }
      }
      if (active) {
        input.preventDefault();
        if (frame === undefined) moved();
      }
    }
    function end(input: PointerEvent) {
      if (input.pointerId !== event.pointerId) return;
      x = input.clientX;
      y = input.clientY;
      if (active) update();
      const destination = active && owns() ? boundary.value : undefined;
      stop();
      if (destination !== undefined) {
        const change = {
          group,
          ids: displayed.map((entry) => entry.id),
          id,
          boundary: destination,
        };
        if (sidebarMoveOrder(change)) options.commit(change);
      }
    }
    cleanup = stop;
    document.addEventListener('pointermove', move, { passive: false });
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', cancel);
    document.addEventListener('keydown', escape, true);
    window.addEventListener('blur', stop);
    root.addEventListener('scroll', moved, { passive: true });
    list.addEventListener('lostpointercapture', cancel);
  }
  watch([options.playlists, options.disabled, options.list, options.root], () => cleanup?.());
  onBeforeUnmount(() => cleanup?.());
  return { boundary, dragging, start, suppressClick: () => performance.now() < suppressUntil };
}
