import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { LocalQueueDropSource, QueueView } from '../services/player';

function suppressReleasedClick(owner: Element, pointerId: number) {
  const row = owner.closest('tr');
  if (!row) return;
  function click(event: MouseEvent) {
    if (!(event.target instanceof Element) || !row!.contains(event.target)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    release();
  }
  function up(event: PointerEvent) {
    if (event.pointerId === pointerId) setTimeout(release, 0);
  }
  function release() {
    document.removeEventListener('click', click, true);
    document.removeEventListener('pointerup', up, true);
    document.removeEventListener('pointercancel', up, true);
    document.removeEventListener('pointerdown', release, true);
    window.removeEventListener('blur', release);
  }
  document.addEventListener('click', click, true);
  document.addEventListener('pointerup', up, true);
  document.addEventListener('pointercancel', up, true);
  document.addEventListener('pointerdown', release, true);
  window.addEventListener('blur', release);
}

/** Capture a typed in-window gesture. There is no HTML/OS data transfer. */
export function useLocalQueueDrop(options: {
  queue(): QueueView | null | undefined;
  playId(): string | null;
  container(): HTMLElement | undefined;
  disabled(): boolean;
  rowHeight: number;
  scroll(): void;
  commit(
    source: LocalQueueDropSource,
    queue: QueueView,
    playId: string | null,
    before: string | null,
  ): void;
}) {
  const boundary = ref<number>();
  const dragging = ref(false);
  const marker = computed(() =>
    boundary.value === undefined ? undefined : boundary.value * options.rowHeight,
  );
  let stop: (() => void) | undefined;

  function begin(event: PointerEvent, source: LocalQueueDropSource) {
    if (event.button !== 0 || event.pointerType === 'touch' || options.disabled()) return;
    stop?.();
    const queue = options.queue();
    const element = options.container();
    const playId = options.playId();
    if (!queue || !queue.tracks.length || !element || !source.current()) return;
    const owner = event.target;
    const owns = () =>
      source.current() &&
      !options.disabled() &&
      element === options.container() &&
      options.queue()?.epoch === queue.epoch &&
      options.queue()?.revision === queue.revision &&
      options.playId() === playId;
    let x = event.clientX,
      y = event.clientY,
      last = 0;
    let frame: number | undefined;
    function update() {
      const box = element!.getBoundingClientRect();
      boundary.value =
        x >= box.left && x <= box.right && y >= box.top && y <= box.bottom
          ? Math.max(
              0,
              Math.min(
                queue!.tracks.length,
                Math.floor(
                  (y - box.top + element!.scrollTop + options.rowHeight / 2) / options.rowHeight,
                ),
              ),
            )
          : undefined;
    }
    function tick(now: number) {
      frame = undefined;
      if (!owns()) return cancel();
      const box = element!.getBoundingClientRect();
      const distance =
        Math.max(0, Math.min(48, y - (box.bottom - 48))) -
        Math.max(0, Math.min(48, box.top + 48 - y));
      if (distance && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom) {
        element!.scrollTop += (distance * Math.min(40, now - (last || now))) / 80;
        options.scroll();
      }
      last = now;
      update();
      frame = requestAnimationFrame(tick);
    }
    function cancel() {
      if (frame !== undefined) cancelAnimationFrame(frame);
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', end);
      document.removeEventListener('pointercancel', pointerCancel);
      document.removeEventListener('keydown', escape, true);
      window.removeEventListener('blur', cancel);
      stop = undefined;
      dragging.value = false;
      boundary.value = undefined;
    }
    function move(e: PointerEvent) {
      if (e.pointerId !== event.pointerId) return;
      if (!(e.buttons & 1) || !owns()) return cancel();
      x = e.clientX;
      y = e.clientY;
      if (!dragging.value && Math.hypot(x - event.clientX, y - event.clientY) >= 6) {
        dragging.value = true;
        // Preserve cancellation even if the pointer returns to the title
        // after Escape and WebView dispatches its compatibility click.
        if (owner instanceof Element) suppressReleasedClick(owner, event.pointerId);
        frame = requestAnimationFrame(tick);
      }
      if (dragging.value) {
        e.preventDefault();
        update();
      }
    }
    function end(e: PointerEvent) {
      if (e.pointerId !== event.pointerId) return;
      const active = dragging.value;
      x = e.clientX;
      y = e.clientY;
      if (active) update();
      const slot = active && owns() ? boundary.value : undefined;
      cancel();
      if (slot !== undefined)
        options.commit(source, queue!, playId, queue!.tracks[slot]?.entryId ?? null);
    }
    function pointerCancel(e: PointerEvent) {
      if (e.pointerId === event.pointerId) cancel();
    }
    function escape(e: KeyboardEvent) {
      if (e.key !== 'Escape') return;
      if (dragging.value) {
        e.preventDefault();
        e.stopImmediatePropagation();
      }
      cancel();
    }
    stop = cancel;
    document.addEventListener('pointermove', move, { passive: false });
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', pointerCancel);
    document.addEventListener('keydown', escape, true);
    window.addEventListener('blur', cancel);
  }
  watch(
    [
      () => options.queue()?.epoch,
      () => options.queue()?.revision,
      options.playId,
      options.disabled,
    ],
    () => stop?.(),
  );
  onBeforeUnmount(() => stop?.());
  return { begin, dragging, marker };
}
