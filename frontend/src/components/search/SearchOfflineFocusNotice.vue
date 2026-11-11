<script setup lang="ts">
import { onUnmounted, ref } from 'vue';
import Icon from '../Icon.vue';

const visible = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;

function pauseDismissal() {
  clearTimeout(timer);
}

function scheduleDismissal() {
  pauseDismissal();
  timer = setTimeout(() => (visible.value = false), 3000);
}

function show() {
  visible.value = true;
  scheduleDismissal();
}

onUnmounted(pauseDismissal);
defineExpose({ show });
</script>

<template>
  <Teleport to="body">
    <div class="search-offline-focus-layer">
      <Transition name="search-offline-focus">
        <div
          v-if="visible"
          class="search-offline-focus-notice"
          data-search-offline-focus
          role="alert"
          @pointerenter="pauseDismissal"
          @pointerleave="scheduleDismissal"
        >
          <Icon name="info" />
          <span>网络异常，请检查网络连接</span>
        </div>
      </Transition>
    </div>
  </Teleport>
</template>

<style scoped>
.search-offline-focus-layer {
  position: fixed;
  z-index: 1010;
  inset: 0;
  display: grid;
  place-items: center;
  pointer-events: none;
}
.search-offline-focus-notice {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  box-sizing: border-box;
  min-height: 44px;
  max-width: min(536px, calc(100% - 24px));
  padding: 0 20px;
  border-radius: 9999px;
  color: #fff;
  background: #2d2d38;
  box-shadow: 0 2px 15px #0002;
  font-size: 14px;
  line-height: 20px;
  pointer-events: auto;
}
.search-offline-focus-notice > .icon {
  width: 18px;
  height: 18px;
  flex: none;
  color: var(--accent);
}
.search-offline-focus-enter-active {
  transition:
    transform 300ms cubic-bezier(0.45, 0.6, 0.58, 0.97),
    opacity 300ms cubic-bezier(0.45, 0.6, 0.58, 0.97);
}
.search-offline-focus-leave-active {
  transition: opacity 200ms ease;
}
.search-offline-focus-enter-from {
  transform: translateY(-100%);
  opacity: 0;
}
.search-offline-focus-leave-to {
  opacity: 0;
}
:global(:root[data-motion='reduced']) .search-offline-focus-enter-active,
:global(:root[data-motion='reduced']) .search-offline-focus-leave-active {
  transition: none;
}
@media (prefers-reduced-motion: reduce) {
  .search-offline-focus-enter-active,
  .search-offline-focus-leave-active {
    transition: none;
  }
}
</style>
