import { watch } from 'vue';

// Capture before either modal mounts, and retain the page origin while a chain
// of related dialogs replaces itself. An inert departing dialog can already
// have dropped focus to body before the next transition's before-enter runs.
export function useModalOrigin(opened: () => boolean, fallback: () => HTMLElement | null) {
  let target: HTMLElement | null = null;
  watch(opened, (current, previous) => {
    if (!current || previous) return;
    const active = document.activeElement;
    target =
      active instanceof HTMLElement && active !== document.body && !active.closest('dialog')
        ? active
        : fallback();
  });
  return { returnFocus: () => target, fallbackFocus: fallback };
}
