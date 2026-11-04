import { nextTick, onScopeDispose, watch, type Ref } from 'vue';

/** Native loading temporarily disables the initiating button in WebKit. */
export function usePlaybackTriggerFocus(
  ready: () => boolean,
  root: Ref<HTMLElement | undefined>,
  ownsVisit: () => boolean,
  settled: () => boolean = ready,
) {
  let origin: HTMLElement | undefined;
  let generation = 0;
  const canRestore = () => ready() && settled();
  async function restore() {
    const current = generation;
    await nextTick();
    if (current !== generation || !canRestore()) return;
    const target = origin;
    origin = undefined;
    if (
      ownsVisit() &&
      target &&
      root.value?.contains(target) &&
      !target.matches(':disabled') &&
      document.activeElement === document.body
    ) {
      const rect = target.getBoundingClientRect();
      let top = Math.max(0, rect.top),
        bottom = Math.min(innerHeight, rect.bottom);
      for (let parent = target.parentElement; parent; parent = parent.parentElement) {
        if (['auto', 'scroll', 'hidden', 'clip'].includes(getComputedStyle(parent).overflowY)) {
          const box = parent.getBoundingClientRect();
          top = Math.max(top, box.top);
          bottom = Math.min(bottom, box.bottom);
        }
      }
      if (bottom - top > 1) target.focus({ preventScroll: true });
    }
  }
  function start(action: () => void, fallback?: HTMLElement | null) {
    if (!ready() || !ownsVisit()) return;
    generation++;
    origin =
      document.activeElement instanceof HTMLElement && root.value?.contains(document.activeElement)
        ? document.activeElement
        : (fallback ?? undefined);
    action();
    void restore();
  }
  watch(
    canRestore,
    (value) => {
      if (value) void restore();
    },
    { flush: 'post' },
  );
  onScopeDispose(() => {
    generation++;
    origin = undefined;
  });
  return { start };
}
