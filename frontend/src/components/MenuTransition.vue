<script setup lang="ts">
function enter(element: Element) {
  (element as HTMLElement).inert = false;
}
function leave(element: Element) {
  (element as HTMLElement).inert = true;
}
</script>
<template>
  <Transition name="menu" @before-enter="enter" @before-leave="leave"><slot /></Transition>
</template>
