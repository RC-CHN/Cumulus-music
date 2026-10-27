<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>['startResizeDragging']>[0];

const available = isTauri();
const maximized = ref(false);
const error = ref('');
let disposed = false;
let unlisten: (() => void) | undefined;
let resizeTimer: ReturnType<typeof setTimeout> | undefined;
const edges: ResizeDirection[] = [
  'North',
  'South',
  'East',
  'West',
  'NorthEast',
  'NorthWest',
  'SouthEast',
  'SouthWest',
];
async function resize(direction: ResizeDirection) {
  try {
    await getCurrentWindow().startResizeDragging(direction);
  } catch {
    error.value = '调整窗口大小失败，请重试';
  }
}
async function refresh() {
  const value = await getCurrentWindow().isMaximized();
  if (!disposed) maximized.value = value;
}
async function action(kind: 'minimize' | 'toggleMaximize' | 'close') {
  error.value = '';
  try {
    await getCurrentWindow()[kind]();
    if (kind === 'toggleMaximize') await refresh();
  } catch {
    error.value = '窗口操作失败，请重试';
  }
}
onMounted(async () => {
  if (!available) return;
  try {
    const stop = await getCurrentWindow().onResized(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        void refresh().catch(() => {});
      }, 80);
    });
    if (disposed) stop();
    else unlisten = stop;
    await refresh();
  } catch {
    error.value = '无法连接窗口控制';
  }
});
onUnmounted(() => {
  disposed = true;
  clearTimeout(resizeTimer);
  unlisten?.();
});
</script>
<template>
  <div
    v-if="available"
    class="window-controls"
    data-tauri-drag-region="false"
    aria-label="窗口控制"
  >
    <span v-if="error" class="window-error" role="alert">{{ error }}</span>
    <template v-if="!maximized">
      <span
        v-for="edge in edges"
        :key="edge"
        class="window-resize"
        :class="`resize-${edge}`"
        aria-hidden="true"
        @mousedown.left.stop.prevent="resize(edge)"
      />
    </template>
    <button aria-label="最小化" title="最小化" @click="action('minimize')">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5h10" /></svg>
    </button>
    <button
      :aria-label="maximized ? '还原窗口' : '最大化'"
      :title="maximized ? '还原窗口' : '最大化'"
      @click="action('toggleMaximize')"
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path v-if="maximized" d="M5.5 5.5v-3h8v8h-3m-8-5h8v8h-8z" />
        <rect v-else x="3.5" y="3.5" width="9" height="9" rx="1" />
      </svg>
    </button>
    <button class="window-close" aria-label="关闭窗口" title="关闭窗口" @click="action('close')">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 4 8 8M12 4l-8 8" /></svg>
    </button>
  </div>
</template>
