import { nextTick, onScopeDispose, watch, type Ref } from 'vue';

/** Native loading temporarily disables the initiating button in WebKit. */
export function useRankingPlaybackFocus(
  ready: () => boolean,
  root: Ref<HTMLElement | undefined>,
  ownsVisit: () => boolean,
) {
  let origin: HTMLElement | undefined;
  let generation = 0;
  async function restore() {
    const current = generation;
    await nextTick();
    if (current !== generation || !ready()) return;
    const target = origin;
    origin = undefined;
    if (
      ownsVisit() &&
      target &&
      root.value?.contains(target) &&
      !target.matches(':disabled') &&
      document.activeElement === document.body
    ) {
      target.focus({ preventScroll: true });
      target.scrollIntoView({ block: 'nearest' });
    }
  }
  function start(action: () => void) {
    if (!ready() || !ownsVisit()) return;
    generation++;
    origin =
      document.activeElement instanceof HTMLElement && root.value?.contains(document.activeElement)
        ? document.activeElement
        : undefined;
    action();
    void restore();
  }
  watch(
    ready,
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
