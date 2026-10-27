<script lang="ts">
// Shared across dialogs so switching from one modal to another still returns
// focus to the original page control when the final modal closes.
const returnTargets = new WeakMap<Element, HTMLElement>();
</script>
<script setup lang="ts">
function entering(element: Element) {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return;
  const parentDialog = active.closest('dialog');
  returnTargets.set(element, (parentDialog && returnTargets.get(parentDialog)) || active);
}
function left(element: Element) {
  const target = returnTargets.get(element);
  returnTargets.delete(element);
  // Native <dialog> keeps the background inert until its leave transition ends.
  // Component onUnmounted runs too early to restore focus during that interval.
  if (!document.querySelector('dialog[open]') && target?.isConnected) target.focus();
}
</script>
<template>
  <Transition name="modal" @before-enter="entering" @after-leave="left"><slot /></Transition>
</template>
