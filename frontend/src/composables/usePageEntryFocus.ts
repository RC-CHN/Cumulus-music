import { nextTick, onMounted, onScopeDispose, watch, type Ref } from 'vue';

/** Focus a newly opened detail after its source control leaves the DOM. */
export function usePageEntryFocus(
  root: Ref<HTMLElement | undefined>,
  ready: () => boolean,
  eligible: () => boolean,
  preferred?: (root: HTMLElement) => HTMLElement | undefined,
) {
  let alive = true,
    handled = false,
    pending = false;
  let origin: Element | null = null;
  async function focus() {
    if (!alive || handled || pending || !root.value || !ready()) return;
    pending = true;
    await nextTick();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await Promise.all(
      (root.value?.closest('.page-view')?.getAnimations() ?? [])
        .filter((animation) => {
          const end = animation.effect?.getComputedTiming().endTime;
          return (
            typeof end === 'number' &&
            Number.isFinite(end) &&
            (animation.playState === 'running' || animation.pending)
          );
        })
        .map((animation) => animation.finished.catch(() => {})),
    );
    pending = false;
    if (!alive || !root.value?.isConnected || root.value.closest('.page-leave-active')) return;
    handled = true;
    if (
      !eligible() ||
      (document.activeElement !== document.body && document.activeElement !== origin)
    )
      return;
    const target =
      preferred?.(root.value) ??
      root.value.querySelector<HTMLElement>('h1') ??
      root.value.querySelector<HTMLElement>('[role="alert"] button:not(:disabled)');
    if (target) {
      if (target.tagName === 'H1') target.tabIndex = -1;
      target.focus({ preventScroll: true });
    }
  }
  onMounted(() => {
    origin = document.activeElement;
    void focus();
  });
  watch(ready, () => void focus(), { flush: 'post' });
  onScopeDispose(() => {
    alive = false;
  });
}
