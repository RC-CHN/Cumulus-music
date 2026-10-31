<script lang="ts">
// Shared across dialogs so switching from one modal to another still returns
// focus to the original page control when the final modal closes.
const returnTargets = new WeakMap<Element, HTMLElement>();
</script>
<script setup lang="ts">
import { onUnmounted } from 'vue';
import { modalEntered, modalLeft } from '../services/modal';
const props = withDefaults(
  defineProps<{
    fallbackFocus?: () => HTMLElement | null;
    returnFocus?: () => HTMLElement | null;
    restoreFocus?: boolean;
  }>(),
  { restoreFocus: true },
);
const emit = defineEmits<{ left: [] }>();
const owned = new Set<Element>();
onUnmounted(() => {
  for (const element of owned) modalLeft(element);
});
function entering(element: Element) {
  (element as HTMLElement).inert = false;
  owned.add(element);
  modalEntered(element);
  const explicit = props.returnFocus?.();
  if (explicit) {
    returnTargets.set(element, explicit);
    return;
  }
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return;
  const parentDialog = active.closest('dialog');
  const replacing = parentDialog?.classList.contains('modal-leave-active');
  returnTargets.set(element, (replacing && returnTargets.get(parentDialog!)) || active);
}
function leaving(element: Element) {
  (element as HTMLElement).inert = true;
}
function left(element: Element) {
  owned.delete(element);
  modalLeft(element);
  const original = returnTargets.get(element);
  const target =
    original?.isConnected && !original.matches(':disabled') ? original : props.fallbackFocus?.();
  returnTargets.delete(element);
  // Native <dialog> keeps the background inert until its leave transition ends.
  // Component onUnmounted runs too early to restore focus during that interval.
  if (
    props.restoreFocus !== false &&
    target?.isConnected &&
    (!document.querySelector('dialog[open]') || target.closest('dialog[open]'))
  ) {
    // Row actions may be hidden until their focusable row gains focus. Reveal
    // that owner first; focusing a display:none action is silently ignored.
    if (!target.getClientRects().length) {
      const owner = target.parentElement?.closest<HTMLElement>('[tabindex]');
      if (owner?.getClientRects().length) owner.focus({ preventScroll: true });
    }
    target.focus({ preventScroll: true });
    if (target !== original) target.scrollIntoView({ block: 'nearest' });
  }
  emit('left');
}
</script>
<template>
  <Transition name="modal" @before-enter="entering" @before-leave="leaving" @after-leave="left"
    ><slot
  /></Transition>
</template>
