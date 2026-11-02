import { watch } from 'vue';

// Capture before either modal mounts, and retain the page origin while a chain
// of related dialogs replaces itself. An inert departing dialog can already
// have dropped focus to body before the next transition's before-enter runs.
export function useModalOrigin(opened: () => boolean, fallback: () => HTMLElement | null) {
  let target: HTMLElement | null = null;
  let restorationPending = false;
  watch(opened, (current, previous) => {
    if (!current && previous) restorationPending = true;
    if (!current || previous) return;
    restorationPending = false;
    const active = document.activeElement;
    target =
      active instanceof HTMLElement && active !== document.body && !active.closest('dialog')
        ? active
        : fallback();
  });
  function restoreIfUnowned() {
    if (
      !restorationPending ||
      opened() ||
      document.querySelector('dialog[open]') ||
      !document.hasFocus()
    )
      return;
    const active = document.activeElement;
    if (
      active instanceof HTMLElement &&
      active !== document.body &&
      active.isConnected &&
      !active.matches(':disabled') &&
      !active.closest('[inert]')
    ) {
      restorationPending = false;
      return;
    }
    const next = target?.isConnected && !target.matches(':disabled') ? target : fallback();
    if (
      next?.isConnected &&
      !next.matches(':disabled') &&
      !next.closest('[inert]') &&
      next.getClientRects().length
    ) {
      next.focus({ preventScroll: true });
      restorationPending = false;
    }
  }
  return { returnFocus: () => target, fallbackFocus: fallback, restoreIfUnowned };
}
