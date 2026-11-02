import { nextTick, onMounted, onScopeDispose, type Ref } from 'vue';
import type { CollectionPosition } from '../services/collectionViewport';

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
    if (restoring || !pending || !root.value || !ready() || !owns()) return;
    restoring = true;
    const ticket = generation,
      position = pending;
    await nextTick();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await Promise.all(
      (root.value?.closest('.page-view')?.getAnimations({ subtree: true }) ?? [])
        .filter((animation) => {
          const end = animation.effect?.getComputedTiming().endTime;
          return (
            typeof end === 'number' &&
            Number.isFinite(end) &&
            (animation.pending || animation.playState === 'running')
          );
        })
        .map((animation) => animation.finished.catch(() => {})),
    );
    if (ticket !== generation) return;
    restoring = false;
    if (!owns() || !root.value?.isConnected || !pending) return;
    const scroll = scroller();
    if (scroll) {
      scroll.scrollTop = position.scrollTop;
      scroll.dispatchEvent(new Event('scroll'));
    }
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await nextTick();
    if (ticket !== generation || !owns() || !root.value?.isConnected) return;
    pending = undefined;
    const active = document.activeElement;
    // Keep a chosen tab/input; a fresh visit may transfer focus from its sidebar/back button.
    if (active !== document.body && (root.value.contains(active) || active !== initialFocus))
      return;
    const target = position.focus
      ? [
          ...root.value.querySelectorAll<HTMLElement>('[data-collection-key],[data-catalog-key]'),
        ].find(
          (node) =>
            (node.dataset.collectionKey ?? node.dataset.catalogKey) === position.focus &&
            !node.matches(':disabled'),
        )
      : undefined;
    (target ?? root.value.querySelector<HTMLElement>('h1'))?.focus({ preventScroll: true });
  }
  onMounted(() => {
    initialFocus = document.activeElement;
    void restore();
  });
  onScopeDispose(cancel);
  return { enter, restore, checkpoint, remember, reset, cancel };
}
