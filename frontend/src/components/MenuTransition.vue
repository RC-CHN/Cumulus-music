<script setup lang="ts">
function enter(element: Element) {
  (element as HTMLElement).inert = false;
}
function leave(element: Element) {
  const active = document.activeElement;
  if (active instanceof HTMLElement && element.contains(active)) active.blur();
  (element as HTMLElement).inert = true;
}
</script>
<template>
  <Transition name="menu" @before-enter="enter" @before-leave="leave"><slot /></Transition>
</template>
