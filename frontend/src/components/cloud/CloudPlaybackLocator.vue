<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue';

const props = defineProps<{ host?: HTMLElement; rowId?: string }>();
const shown = ref(false);
const button = ref<HTMLButtonElement>();
let hideTimer: ReturnType<typeof setTimeout> | undefined;
let hovered = false;
let keyboardFocus = false;

function clearTimer() {
  if (hideTimer !== undefined) clearTimeout(hideTimer);
  hideTimer = undefined;
}
function reset() {
  clearTimer();
  shown.value = hovered = keyboardFocus = false;
}
function hideLater() {
  clearTimer();
  if (!shown.value || hovered || keyboardFocus) return;
  hideTimer = setTimeout(reset, 3_000);
}
function scrolled() {
  if (!props.rowId) return;
  shown.value = true;
  hideLater();
}
function hover(active: boolean) {
  hovered = active;
  hideLater();
}
function focus(active: boolean) {
  keyboardFocus = active && !!button.value?.matches(':focus-visible');
  hideLater();
}
function locate() {
  const { host, rowId } = props;
  if (!shown.value || !host?.isConnected || !rowId) return;
  const row = host.querySelector<HTMLElement>('.cloud-table tr.playing[data-cloud-id]');
  if (row?.dataset.cloudId !== rowId) return;
  const bounds = row.getBoundingClientRect();
  const top = host.getBoundingClientRect().top + host.clientTop;
  host.scrollTop = Math.floor(
    host.scrollTop + bounds.top - top + (bounds.height - host.clientHeight) / 2,
  );
}

watch(
  () => props.host,
  (host, _, onCleanup) => {
    reset();
    host?.addEventListener('scroll', scrolled, { passive: true });
    onCleanup(() => host?.removeEventListener('scroll', scrolled));
  },
  { immediate: true },
);
watch(
  () => props.rowId,
  () => {
    if (document.activeElement === button.value) props.host?.focus({ preventScroll: true });
    reset();
  },
);
onUnmounted(reset);
</script>

<template>
  <Transition name="cloud-locator">
    <button
      v-if="shown && rowId"
      ref="button"
      class="cloud-playback-locator"
      type="button"
      aria-label="定位当前播放"
      title="定位当前播放"
      @pointerenter="hover(true)"
      @pointerleave="hover(false)"
      @focus="focus(true)"
      @blur="focus(false)"
      @click="locate"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="7" />
        <circle cx="12" cy="12" r="2" />
        <path d="M12 2v3m0 14v3M2 12h3m14 0h3" />
      </svg>
    </button>
  </Transition>
</template>

<style scoped>
.cloud-playback-locator {
  position: fixed;
  z-index: 4;
  right: 42px;
  bottom: 122px;
  display: grid;
  width: 46px;
  height: 46px;
  place-items: center;
  padding: 0;
  border: 0;
  border-radius: 50%;
  color: var(--text-secondary);
  background: var(--surface);
  box-shadow: 0 2px 17px rgb(0 0 0 / 14%);
  transition:
    opacity 140ms ease,
    transform 140ms ease;
}
.cloud-playback-locator svg {
  width: 28px;
  height: 28px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
}
.cloud-playback-locator:hover {
  transform: scale(1.0435);
}
.cloud-playback-locator:active {
  opacity: 0.8;
  transform: scale(0.96);
}
.cloud-playback-locator:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.cloud-locator-enter-from,
.cloud-locator-leave-to {
  opacity: 0;
}
.cloud-locator-leave-active {
  pointer-events: none;
}
@media (prefers-reduced-motion: reduce) {
  .cloud-playback-locator {
    transition: none;
  }
  .cloud-playback-locator:hover,
  .cloud-playback-locator:active {
    transform: none;
  }
}
</style>
