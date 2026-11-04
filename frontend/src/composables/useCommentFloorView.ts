import { nextTick, onScopeDispose, watch, type Ref } from 'vue';
import type { CommentItem } from '../services/comments';
import type { CommentFloorModel } from './useCommentFloor';
import { useMotion } from './useMotion';
import { visibleCommentControl } from './useCommentReadFocus';

/** Presentation ownership for an inline floor; the read model keeps all cursor state. */
export function useCommentFloorView(
  model: CommentFloorModel,
  root: Ref<HTMLElement | undefined>,
  panel: Ref<HTMLDivElement | undefined>,
  scope: () => string,
  origin: Ref<{ group: string; commentId: string } | undefined>,
) {
  let serial = 0;
  const motion = useMotion();
  const stop = watch(
    scope,
    () => {
      serial++;
    },
    { flush: 'sync' },
  );
  onScopeDispose(() => {
    serial++;
    stop();
  });
  async function open(item: CommentItem, event: MouseEvent) {
    const ticket = ++serial,
      owner = scope(),
      node = root.value;
    const anchor = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined;
    const group = anchor?.closest<HTMLElement>('.comment-group')?.dataset.group;
    origin.value = group ? { group, commentId: item.id } : undefined;
    const pending = model.open(item);
    await nextTick();
    if (
      ticket === serial &&
      owner === scope() &&
      node === root.value &&
      node?.isConnected &&
      model.query.value?.parentId === (item.parentId ?? item.id) &&
      (document.activeElement === document.body || document.activeElement === anchor)
    ) {
      panel.value
        ?.querySelector<HTMLElement>('[data-comment-read-heading]')
        ?.focus({ preventScroll: true });
      panel.value?.scrollIntoView({
        block: 'start',
        behavior: motion.reduced.value ? 'instant' : 'smooth',
      });
    }
    await pending;
  }
  async function close() {
    const owner = scope(),
      node = root.value,
      address = origin.value;
    origin.value = undefined;
    serial++;
    model.close();
    await nextTick();
    if (
      owner !== scope() ||
      node !== root.value ||
      !node?.isConnected ||
      document.activeElement !== document.body
    )
      return;
    const anchor =
      address &&
      Array.from(node.querySelectorAll<HTMLButtonElement>('[data-comment-floor-trigger]')).find(
        (button) =>
          button.closest<HTMLElement>('.comment-group')?.dataset.group === address.group &&
          button.closest<HTMLElement>('[data-comment-id]')?.dataset.commentId === address.commentId,
      );
    if (anchor && visibleCommentControl(anchor)) anchor.focus({ preventScroll: true });
    else if (anchor) {
      anchor.focus({ preventScroll: true });
      anchor.scrollIntoView({ block: 'nearest' });
    } else node.querySelector<HTMLElement>('[data-comment-read-heading]')?.focus();
  }
  return { open, close };
}
