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
) {
  let serial = 0,
    origin: HTMLElement | undefined;
  const motion = useMotion();
  const stop = watch(
    scope,
    () => {
      serial++;
      origin = undefined;
    },
    { flush: 'sync' },
  );
  onScopeDispose(() => {
    serial++;
    origin = undefined;
    stop();
  });
  async function open(item: CommentItem, event: MouseEvent) {
    const ticket = ++serial,
      owner = scope(),
      node = root.value;
    origin = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined;
    const anchor = origin,
      pending = model.open(item);
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
      anchor = origin;
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
    if (visibleCommentControl(anchor)) anchor!.focus({ preventScroll: true });
    else if (anchor?.isConnected) anchor.focus();
    else node.querySelector<HTMLElement>('[data-comment-read-heading]')?.focus();
  }
  return { open, close };
}
