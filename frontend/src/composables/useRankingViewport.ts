import { nextTick, onMounted, onScopeDispose, watch, type Ref } from 'vue';
import type { useRankings } from './useRankings';
import type { RankingUiState } from '../services/rankings';
import { waitForViewportAnimations, waitForViewportFrames } from '../services/viewportLayout.ts';

/** A history entry owns its reading position; refreshes never replay a focus move. */
export function useRankingViewport(
  model: ReturnType<typeof useRankings>,
  root: Ref<HTMLElement | undefined>,
) {
  const owns = model.captureVisit();
  let lastFocus: RankingUiState['focus'] = null;
  let scroll: HTMLElement | null = null;
  let alive = true,
    restored = false,
    restoring = false,
    userMoved = false;
  let initialFocus: Element | null = null;
  let managedFocus: Element | null = null;
  const moved = () => (userMoved = true);
  function resource(target: EventTarget | null): RankingUiState['focus'] {
    if (!(target instanceof Element) || !root.value?.contains(target)) return null;
    const control = target.closest<HTMLElement>('[data-rank-action]');
    const action = control?.dataset.rankAction as NonNullable<RankingUiState['focus']>['action'];
    const card = target.closest<HTMLElement>('[data-rank-id]');
    if (card) return { kind: 'card', id: card.dataset.rankId!, action };
    const album = target.closest<HTMLElement>('[data-sale-id]');
    if (album) return { kind: 'album', id: album.dataset.saleId!, action };
    const song = target.closest<HTMLElement>('[data-sale-song-id]');
    return song ? { kind: 'song', id: song.dataset.saleSongId!, action } : null;
  }
  function remember(target: EventTarget | null) {
    lastFocus = resource(target) ?? lastFocus;
  }
  function checkpoint() {
    return model.checkpoint({
      scrollTop: scroll?.scrollTop ?? 0,
      focus: resource(document.activeElement) ?? lastFocus,
    });
  }
  async function restore() {
    const saved = model.restoration.value,
      element = root.value;
    // A remount can still show cached cards while the same-account read
    // adds a loading row above them. Do not consume the saved position in
    // that transient layout: removing the row can shift or clamp scroll.
    if (!alive || !element || !owns() || restoring || restored || (saved && model.loading.value))
      return;
    restoring = true;
    const valid = () => alive && owns() && root.value === element && element.isConnected;
    try {
      await nextTick();
      await waitForViewportFrames(saved ? 2 : 1);
      if (!valid()) return;
      await waitForViewportAnimations(element.closest('.page-view'), false);
      if (!valid() || !scroll || (saved && model.loading.value)) return;
      const contentReady = !!model.view.value || !!model.error.value;
      restored = contentReady;
      if (saved && contentReady && !userMoved) scroll.scrollTop = saved.scrollTop;
      else if (!saved && !userMoved) scroll.scrollTop = 0;
      if (
        document.activeElement !== document.body &&
        document.activeElement !== initialFocus &&
        document.activeElement !== managedFocus
      )
        return;
      const selector =
        saved?.focus?.kind === 'card'
          ? '[data-rank-id]'
          : saved?.focus?.kind === 'song'
            ? '[data-sale-song-id]'
            : '[data-sale-id]';
      const target = [...element.querySelectorAll<HTMLElement>(selector)].find(
        (node) =>
          (node.dataset.rankId ?? node.dataset.saleId ?? node.dataset.saleSongId) ===
          saved?.focus?.id,
      );
      const control =
        target &&
        [...target.querySelectorAll<HTMLElement>('button:not(:disabled)')].find(
          (node) => node.dataset.rankAction === saved?.focus?.action,
        );
      const focus =
        control ??
        target?.querySelector<HTMLElement>('button:not(:disabled)') ??
        element.querySelector<HTMLElement>('h1');
      focus?.focus({ preventScroll: true });
      managedFocus = focus ?? null;
      lastFocus = saved?.focus ?? null;
    } finally {
      restoring = false;
    }
  }
  onMounted(() => {
    scroll = root.value?.closest<HTMLElement>('.main-content') ?? null;
    initialFocus = document.activeElement;
    scroll?.addEventListener('wheel', moved, { passive: true });
    scroll?.addEventListener('touchmove', moved, { passive: true });
    void restore();
  });
  watch([model.view, model.error, model.loading], () => void restore(), { flush: 'post' });
  onScopeDispose(() => {
    alive = false;
    scroll?.removeEventListener('wheel', moved);
    scroll?.removeEventListener('touchmove', moved);
  });
  return { remember, checkpoint };
}
