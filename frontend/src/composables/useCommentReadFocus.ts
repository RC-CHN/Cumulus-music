import { nextTick, onScopeDispose, type Ref } from 'vue';

export function visibleCommentControl(node: HTMLElement | undefined): boolean {
  if (!node?.isConnected || node.matches(':disabled')) return false;
  const rect = node.getBoundingClientRect();
  let left = Math.max(0, rect.left),
    right = Math.min(innerWidth, rect.right),
    top = Math.max(0, rect.top),
    bottom = Math.min(innerHeight, rect.bottom);
  for (let parent: HTMLElement | null = node; parent; parent = parent.parentElement) {
    const style = getComputedStyle(parent),
      box = parent.getBoundingClientRect();
    if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0)
      return false;
    if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowY)) {
      top = Math.max(top, box.top);
      bottom = Math.min(bottom, box.bottom);
    }
    if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowX)) {
      left = Math.max(left, box.left);
      right = Math.min(right, box.right);
    }
  }
  return right - left > 1 && bottom - top > 1;
}

/** Restore a read control without taking focus from a newer user choice or resource. */
export function useCommentReadFocus(
  root: Ref<HTMLElement | undefined>,
  scope: () => string,
  position: () => string,
) {
  let serial = 0;
  onScopeDispose(() => {
    serial++;
  });
  return async (event: Event, action: () => unknown, preferred?: HTMLElement) => {
    const ticket = ++serial,
      owner = scope(),
      previousPosition = position(),
      node = root.value;
    const origin =
      preferred ??
      (event.currentTarget instanceof HTMLButtonElement
        ? event.currentTarget
        : document.activeElement instanceof HTMLElement
          ? document.activeElement
          : undefined);
    const region =
      origin?.closest<HTMLElement>(
        '.comment-pagination,.comment-floor-panel footer,.comments-tabs,.comments-tools',
      ) ?? node;
    const pending = action();
    await nextTick();
    let interim: HTMLElement | undefined;
    if (
      ticket === serial &&
      scope() === owner &&
      root.value === node &&
      node?.isConnected &&
      origin &&
      (!origin.isConnected || origin.matches(':disabled')) &&
      (document.activeElement === document.body || document.activeElement === origin)
    ) {
      interim = region;
      if (interim && visibleCommentControl(interim)) {
        interim.tabIndex = -1;
        interim.focus({ preventScroll: true });
        interim.scrollIntoView({ block: 'nearest' });
      }
    }
    await pending;
    await nextTick();
    if (
      ticket !== serial ||
      scope() !== owner ||
      root.value !== node ||
      !node?.isConnected ||
      node.closest('.page-leave-active,.catalog-tab-leave-active')
    )
      return;
    const active = document.activeElement;
    if (active !== document.body && active !== origin && active !== interim) return;
    if (visibleCommentControl(origin)) {
      origin!.focus({ preventScroll: true });
      origin!.scrollIntoView({ block: 'nearest' });
    } else if (position() === previousPosition) {
      const retained = visibleCommentControl(interim) ? interim! : node;
      retained.tabIndex = -1;
      retained.focus({ preventScroll: true });
      retained.scrollIntoView({ block: 'nearest' });
    } else node.querySelector<HTMLElement>('[data-comment-read-heading]')?.focus();
  };
}
