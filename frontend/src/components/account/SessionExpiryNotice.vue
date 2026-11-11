<script setup lang="ts">
defineProps<{ message?: string }>();
</script>

<template>
  <Teleport to="body">
    <Transition name="session-expiry">
      <div v-if="message" class="session-expiry-toast" data-live-jsk-expiry role="alert">
        {{ message }}
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.session-expiry-toast {
  position: fixed;
  z-index: 1010;
  inset-block-start: 72px;
  inset-inline: 0;
  box-sizing: border-box;
  inline-size: max-content;
  max-inline-size: min(540px, calc(100vw - 24px));
  margin-inline: auto;
  padding: 11px 18px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface);
  box-shadow: var(--shadow);
  color: var(--text);
  overflow-wrap: anywhere;
  pointer-events: none;
}
.session-expiry-enter-active,
.session-expiry-leave-active {
  transition: opacity 140ms ease;
}
.session-expiry-enter-from,
.session-expiry-leave-to {
  opacity: 0;
}
:global(:root[data-motion='reduced']) .session-expiry-enter-active,
:global(:root[data-motion='reduced']) .session-expiry-leave-active {
  transition: none;
}
@media (prefers-reduced-motion: reduce) {
  .session-expiry-enter-active,
  .session-expiry-leave-active {
    transition: none;
  }
}
</style>
