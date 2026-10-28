<script lang="ts">
// Shared across dialogs so switching from one modal to another still returns
// focus to the original page control when the final modal closes.
const returnTargets = new WeakMap<Element, HTMLElement>();
</script>
<script setup lang="ts">
import { onUnmounted } from 'vue';
import { modalEntered, modalLeft } from '../services/modal';
const owned = new Set<Element>();
onUnmounted(() => {
  for (const element of owned) modalLeft(element);
});
function entering(element: Element) {
  owned.add(element);
  modalEntered(element);
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return;
  const parentDialog = active.closest('dialog');
  const replacing = parentDialog?.classList.contains('modal-leave-active');
  returnTargets.set(element, (replacing && returnTargets.get(parentDialog!)) || active);
}
function left(element: Element) {
  owned.delete(element);
  modalLeft(element);
  const target = returnTargets.get(element);
  returnTargets.delete(element);
  // Native <dialog> keeps the background inert until its leave transition ends.
  // Component onUnmounted runs too early to restore focus during that interval.
  if (
    target?.isConnected &&
    (!document.querySelector('dialog[open]') || target.closest('dialog[open]'))
  )
    target.focus({ preventScroll: true });
}
</script>
<template>
  <Transition name="modal" @before-enter="entering" @after-leave="left"><slot /></Transition>
</template>
