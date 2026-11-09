import { onBeforeUnmount, ref, watch } from 'vue';
import { suppressReleasedClick } from './useQueueDrop';

export function useMixedPlaylistDrag(options: {
  container(): HTMLElement | undefined;
  scope(): object | undefined;
  boundary(offset: number): number;
  scroll(): void;
}) {
  const gap = ref<number>();
  let cleanup: (() => void) | undefined;

  function start(event: PointerEvent, current: () => boolean, commit: (gap: number) => void) {
    cleanup?.();
    const node = options.container();
    const scope = options.scope();
    if (!scope || !node || !current()) return;
    let active = false;
    let frame: number | undefined;
    let previousTime = 0;
    let x = event.clientX;
    let y = event.clientY;
    const owns = () =>
      scope === options.scope() && node === options.container() && node.isConnected && current();
    function over(box: DOMRect) {
      if (x < box.left || x >= box.right || y < box.top || y >= box.bottom) return false;
      const hit = document.elementFromPoint(x, y);
      return !!hit && node!.contains(hit);
    }
    function update() {
      const box = node!.getBoundingClientRect();
      gap.value = over(box) ? options.boundary(y - box.top + node!.scrollTop) : undefined;
    }
    function tick(now: number) {
      frame = undefined;
      if (!owns()) return stop();
      const box = node!.getBoundingClientRect();
      const distance =
        Math.max(0, Math.min(48, y - (box.bottom - 48))) -
        Math.max(0, Math.min(48, box.top + 48 - y));
      if (distance && over(box)) {
        node!.scrollTop += (distance * Math.min(40, now - (previousTime || now))) / 80;
        options.scroll();
      }
      previousTime = now;
      update();
      frame = requestAnimationFrame(tick);
    }
    function stop() {
      if (frame !== undefined) cancelAnimationFrame(frame);
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', end);
      document.removeEventListener('pointercancel', cancel);
      document.removeEventListener('keydown', escape, true);
      window.removeEventListener('blur', stop);
      gap.value = undefined;
      cleanup = undefined;
    }
    function move(e: PointerEvent) {
      if (e.pointerId !== event.pointerId) return;
      if (!(e.buttons & 1) || !owns()) return stop();
      x = e.clientX;
      y = e.clientY;
      if (!active && Math.hypot(x - event.clientX, y - event.clientY) >= 6) {
        active = true;
        if (event.target instanceof Element) suppressReleasedClick(event.target, event.pointerId);
        frame = requestAnimationFrame(tick);
      }
      if (active) {
        e.preventDefault();
        update();
      }
    }
    function end(e: PointerEvent) {
      if (e.pointerId !== event.pointerId) return;
      x = e.clientX;
      y = e.clientY;
      if (active) update();
      const target = active && owns() ? gap.value : undefined;
      stop();
      if (target !== undefined) commit(target);
    }
    function cancel(e: PointerEvent) {
      if (e.pointerId === event.pointerId) stop();
    }
    function escape(e: KeyboardEvent) {
      if (e.key !== 'Escape') return;
      if (active) {
        e.preventDefault();
        // The queue's independent destination listener must also cancel.
        e.stopPropagation();
      }
      stop();
    }
    cleanup = stop;
    document.addEventListener('pointermove', move, { passive: false });
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', cancel);
    document.addEventListener('keydown', escape, true);
    window.addEventListener('blur', stop);
  }

  watch(options.scope, () => cleanup?.(), { flush: 'sync' });
  onBeforeUnmount(() => cleanup?.());
  return { gap, start };
}
