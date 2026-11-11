<script setup lang="ts">
defineProps<{ notices: readonly string[] }>();
const emit = defineEmits<{ hover: [id: string, entered: boolean] }>();
</script>

<template>
  <Teleport to="body">
    <TransitionGroup tag="div" name="ordinary-skip-toast" class="ordinary-skip-layer">
      <div
        v-for="id in notices"
        :key="id"
        class="ordinary-skip-notice"
        role="status"
        @mouseenter="emit('hover', id, true)"
        @mouseleave="emit('hover', id, false)"
      >
        <span>已为您自动跳过不能播放的歌曲</span>
      </div>
    </TransitionGroup>
  </Teleport>
</template>

<style scoped>
.ordinary-skip-layer {
  position: fixed;
  z-index: 1010;
  inset-inline: 0;
  top: 50%;
  display: flex;
  flex-direction: column;
  align-items: center;
  pointer-events: none;
  transform: translateY(-50%);
}

.ordinary-skip-notice {
  box-sizing: border-box;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 44px;
  max-width: min(536px, calc(100vw - 24px));
  padding-inline: 20px;
  border-radius: 9999px;
  box-shadow: 0 2px 15px rgb(0 0 0 / 10%);
  color: #fff;
  background: #2d2d38;
  font-size: 14px;
  font-weight: 400;
  line-height: 20px;
  pointer-events: auto;
  text-align: center;
}

.ordinary-skip-toast-enter-active {
  animation: ordinary-skip-toast-in 300ms cubic-bezier(0.45, 0.6, 0.58, 0.97);
}

.ordinary-skip-toast-leave-active {
  animation: ordinary-skip-toast-out 200ms cubic-bezier(0.45, 0.6, 0.58, 0.97);
}

@keyframes ordinary-skip-toast-in {
  from {
    opacity: 0;
    transform: translateY(-100%);
  }
  to {
    opacity: 1;
  }
}

@keyframes ordinary-skip-toast-out {
  from {
    opacity: 1;
  }
  to {
    opacity: 0;
    transform: translateY(-100%);
  }
}

:global(:root[data-motion='reduced']) .ordinary-skip-toast-enter-active,
:global(:root[data-motion='reduced']) .ordinary-skip-toast-leave-active {
  animation: none;
}

@media (prefers-reduced-motion: reduce) {
  .ordinary-skip-toast-enter-active,
  .ordinary-skip-toast-leave-active {
    animation: none;
  }
}
</style>
