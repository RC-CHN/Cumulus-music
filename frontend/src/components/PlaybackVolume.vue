<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { usePlaybackVolume } from '../composables/usePlaybackVolume';
import Icon from './Icon.vue';
import MenuTransition from './MenuTransition.vue';
const props = defineProps<{ player: PlayerModel }>();
const model = usePlaybackVolume(props.player);
const opened = ref(false),
  root = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>(),
  retryButton = ref<HTMLButtonElement>(),
  slider = ref<HTMLInputElement>();
const id = useId();
let hide: ReturnType<typeof setTimeout> | undefined,
  suppressed = false;
function show() {
  if (!props.player.ready.value) return;
  clearTimeout(hide);
  suppressed = false;
  opened.value = true;
}
function close(restore = false) {
  clearTimeout(hide);
  suppressed = true;
  opened.value = false;
  model.end();
  if (restore) trigger.value?.focus({ preventScroll: true });
}
function leave() {
  clearTimeout(hide);
  hide = setTimeout(() => {
    if (!root.value?.contains(document.activeElement) && !model.dragging.value) close();
  }, 120);
}
function focus(event: FocusEvent) {
  if (root.value?.contains(event.relatedTarget as Node)) return;
  if (!suppressed) show();
}
function keydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && opened.value) {
    event.preventDefault();
    event.stopPropagation();
    close(true);
  } else if (event.target === trigger.value && ['ArrowUp', 'ArrowDown'].includes(event.key)) {
    event.preventDefault();
    show();
    void nextTick(() => slider.value?.focus({ preventScroll: true }));
  }
}
function outside(event: PointerEvent) {
  if (!root.value?.contains(event.target as Node)) close();
}
function begin(event: PointerEvent) {
  model.begin();
  if (event.isTrusted) (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}
watch(model.issue, async (issue, previous) => {
  const lostRetry = previous && !issue && document.activeElement === retryButton.value;
  if (!lostRetry) return;
  await nextTick();
  if (opened.value && !root.value?.contains(document.activeElement))
    slider.value?.focus({ preventScroll: true });
});
watch([() => props.player.view.value?.epoch, props.player.ready], () => close(), { flush: 'sync' });
onMounted(() => document.addEventListener('pointerdown', outside, true));
onUnmounted(() => {
  clearTimeout(hide);
  document.removeEventListener('pointerdown', outside, true);
});
</script>
<template>
  <div
    ref="root"
    class="volume-control"
    @pointerenter="show"
    @pointerleave="leave"
    @focusin="focus"
    @focusout="leave"
    @keydown="keydown"
    @wheel="
      (event) => {
        if (opened && player.ready.value) {
          event.preventDefault();
          model.wheel(event.deltaY);
        }
      }
    "
  >
    <button
      ref="trigger"
      class="icon-button volume-trigger"
      :aria-label="model.value.value === 0 ? '取消静音' : '静音'"
      :title="`${model.value.value === 0 ? '取消静音' : '静音'} · 音量 ${model.value.value}%`"
      :aria-controls="opened ? id : undefined"
      :aria-expanded="opened"
      :disabled="!player.ready.value"
      @click="model.toggleMute"
    >
      <Icon :name="model.value.value === 0 ? 'muted' : 'volume'" />
    </button>
    <MenuTransition>
      <div
        v-if="opened"
        :id="id"
        class="volume-popover"
        :class="{ 'volume-error': model.issue.value }"
        role="group"
        aria-label="音量调节"
        :aria-busy="model.pending.value"
      >
        <div class="volume-slider-area">
          <input
            ref="slider"
            aria-label="音量"
            aria-orientation="vertical"
            :aria-valuetext="`${model.value.value}%`"
            type="range"
            min="0"
            max="100"
            step="1"
            :value="model.value.value"
            :style="{ '--volume-percent': `${model.value.value}%` }"
            :disabled="!player.ready.value"
            @pointerdown="begin"
            @pointerup="model.end"
            @pointercancel="model.end"
            @lostpointercapture="model.end"
            @input="model.set(Number(($event.target as HTMLInputElement).value))"
            @change="model.set(Number(($event.target as HTMLInputElement).value))"
            @blur="model.end"
          />
        </div>
        <output class="volume-percentage">{{ model.value.value }}%</output>
        <div v-if="model.issue.value" class="volume-feedback">
          <p role="alert">{{ model.issue.value.message }}</p>
          <button
            ref="retryButton"
            v-if="model.canRetry.value"
            class="text-button"
            @click="model.retry"
          >
            重试音量
          </button>
        </div>
      </div>
    </MenuTransition>
  </div>
</template>
