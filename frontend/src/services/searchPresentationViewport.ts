/** Retain a live search viewport across presentation-only keyed DOM updates. */
export function captureSearchPresentationViewport(panel: HTMLElement) {
  const active = document.activeElement as HTMLElement | null;
  const focused = active && panel.contains(active) ? active : undefined;
  const card = focused?.closest<HTMLElement>('[data-result-key]');
  const section = card?.closest<HTMLElement>('[data-result-section]');
  return {
    top: panel.scrollTop,
    focused,
    cardKey: card?.dataset.resultKey,
    sectionKey: section?.dataset.resultSection,
    action: focused?.dataset.resultAction,
  };
}
export function restoreSearchPresentationViewport(
  panel: HTMLElement,
  saved: ReturnType<typeof captureSearchPresentationViewport>,
) {
  // A shorter layout may naturally clamp to its new maximum scroll offset.
  panel.scrollTop = saved.top;
  if (!saved.focused || saved.focused.isConnected || document.activeElement !== document.body)
    return;
  const section = [...panel.querySelectorAll<HTMLElement>('[data-result-section]')].find(
    (node) => node.dataset.resultSection === saved.sectionKey,
  );
  const cards = [...(section ?? panel).querySelectorAll<HTMLElement>('[data-result-key]')];
  const card = cards.find((node) => node.dataset.resultKey === saved.cardKey) ?? cards[0];
  const controls = [
    ...(card?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
  ].filter((node) => node.getClientRects().length && !node.closest('[inert]'));
  const target =
    controls.find((node) => saved.action && node.dataset.resultAction === saved.action) ??
    controls[0];
  target?.focus({ preventScroll: true });
}
