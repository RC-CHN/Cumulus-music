import { nextTick, onMounted, onScopeDispose, type Ref } from 'vue';
import type { CollectionPosition } from '../services/collectionViewport';
import { waitForViewportAnimations, waitForViewportFrames } from '../services/viewportLayout.ts';

/** A visit owns reading position; late reads never scroll over a user's new input. */
export function useCollectionViewport(
  root: Ref<HTMLElement | undefined>,
  ready: () => boolean,
  owns: () => boolean,
) {
  let pending: CollectionPosition | undefined;
  let lastFocus: string | undefined;
  let initialFocus: Element | null = null;
  let generation = 0;
  let restoring = false;
  const scroller = () =>
    root.value?.querySelector<HTMLElement>('.catalog-card-scroll,.collection-follow-scroll');
  function key(target: EventTarget | null) {
    if (!(target instanceof Element) || !root.value?.contains(target)) return;
    const node = target.closest<HTMLElement>('[data-collection-key],[data-catalog-key]');
    return node?.dataset.collectionKey ?? node?.dataset.catalogKey;
  }
  function remember(event: FocusEvent) {
    lastFocus = key(event.target) ?? lastFocus;
  }
  function checkpoint(): CollectionPosition {
    return (
      pending ?? {
        scrollTop: scroller()?.scrollTop ?? 0,
        focus: key(document.activeElement) ?? lastFocus,
      }
    );
  }
  function cancel() {
    generation++;
    pending = undefined;
    restoring = false;
  }
  function reset() {
    cancel();
    const node = scroller();
    if (node) {
      node.scrollTop = 0;
      node.dispatchEvent(new Event('scroll'));
    }
    lastFocus = key(document.activeElement);
  }
  function enter(position?: CollectionPosition) {
    generation++;
    restoring = false;
    pending = position ? { ...position } : { scrollTop: 0 };
    lastFocus = position?.focus;
    initialFocus = document.activeElement;
    void restore();
  }
  async function restore() {
    const element = root.value;
    if (restoring || !pending || !element || !ready() || !owns()) return;
    restoring = true;
    const ticket = generation,
      position = pending;
    const valid = () =>
      ticket === generation &&
      pending === position &&
      owns() &&
      root.value === element &&
      element.isConnected;
    try {
      await nextTick();
      await waitForViewportFrames();
      if (!valid()) return;
      await waitForViewportAnimations(element.closest('.page-view'));
      if (!valid()) return;
      const scroll = scroller();
      if (scroll) {
        scroll.scrollTop = position.scrollTop;
        scroll.dispatchEvent(new Event('scroll'));
      }
      await waitForViewportFrames();
      await nextTick();
      if (!valid()) return;
      pending = undefined;
      const active = document.activeElement;
      // Keep a chosen tab/input; a fresh visit may transfer focus from its sidebar/back button.
      if (active !== document.body && (element.contains(active) || active !== initialFocus)) return;
      const target = position.focus
        ? [
            ...element.querySelectorAll<HTMLElement>('[data-collection-key],[data-catalog-key]'),
          ].find(
            (node) =>
              (node.dataset.collectionKey ?? node.dataset.catalogKey) === position.focus &&
              !node.matches(':disabled'),
          )
        : undefined;
      (target ?? element.querySelector<HTMLElement>('h1'))?.focus({ preventScroll: true });
    } finally {
      // A cancelled restore must not unlock a newer visit.
      if (ticket === generation) restoring = false;
    }
  }
  onMounted(() => {
    initialFocus = document.activeElement;
    void restore();
  });
  onScopeDispose(cancel);
  return { enter, restore, checkpoint, remember, reset, cancel };
}
