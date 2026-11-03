<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, useId } from 'vue';
import MenuTransition from './MenuTransition.vue';

const size = defineModel<number>({ required: true });
const opened = ref(false);
const root = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const slider = ref<HTMLInputElement>();
const id = useId();

function setSize(value: number) {
  if (Number.isInteger(value)) size.value = Math.max(-4, Math.min(4, value));
}
function toggle() {
  opened.value = !opened.value;
  if (opened.value) void nextTick(() => slider.value?.focus({ preventScroll: true }));
}
function close(restore: boolean) {
  opened.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
function outside(event: PointerEvent) {
  if (!root.value?.contains(event.target as Node)) close(false);
}
function blur(event: FocusEvent) {
  if (event.relatedTarget && !root.value?.contains(event.relatedTarget as Node)) close(false);
}
function escape(event: KeyboardEvent) {
  if (!opened.value) return;
  event.stopPropagation();
  event.preventDefault();
  close(true);
}
onMounted(() => document.addEventListener('pointerdown', outside, true));
onUnmounted(() => document.removeEventListener('pointerdown', outside, true));
</script>

<template>
  <div ref="root" class="lyric-font-control" @focusout="blur" @keydown.esc="escape">
    <button
      ref="trigger"
      type="button"
      class="text-button"
      title="调整字号"
      aria-label="调整歌词字号"
      :aria-controls="opened ? id : undefined"
      :aria-expanded="opened"
      @click="toggle"
    >
      字号
    </button>
    <MenuTransition>
      <div v-if="opened" :id="id" class="lyric-font-popover" role="group" aria-label="歌词字号">
        <button
          type="button"
          class="icon-button"
          aria-label="增大歌词字号"
          :aria-disabled="size >= 4"
          @click="setSize(size + 1)"
        >
          +
        </button>
        <div class="lyric-font-slider">
          <input
            ref="slider"
            type="range"
            min="-4"
            max="4"
            step="1"
            aria-label="歌词字号增量"
            aria-orientation="vertical"
            :aria-valuetext="`${size > 0 ? '+' : ''}${size}`"
            :value="size"
            @input="setSize(Number(($event.target as HTMLInputElement).value))"
          />
          <output aria-hidden="true">{{ size > 0 ? '+' : '' }}{{ size }}</output>
        </div>
        <button
          type="button"
          class="icon-button"
          aria-label="减小歌词字号"
          :aria-disabled="size <= -4"
          @click="setSize(size - 1)"
        >
          −
        </button>
      </div>
    </MenuTransition>
  </div>
</template>
