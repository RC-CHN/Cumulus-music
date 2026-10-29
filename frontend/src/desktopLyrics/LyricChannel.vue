<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { DesktopLyricWord } from '../services/desktopLyrics';
import LyricInk from './LyricInk.vue';
const props = defineProps<{
  text: string;
  words?: DesktopLyricWord[];
  active: boolean;
  progress: number;
  vertical: boolean;
  align: 'left' | 'center' | 'right';
  auxiliary?: boolean;
}>();
const viewport = ref<HTMLElement>(),
  text = ref<HTMLElement>(),
  overflow = ref(0);
const position = computed(() => -overflow.value * props.progress);
const offset = computed(() =>
  props.vertical ? `translateY(${position.value}px)` : `translateX(${position.value}px)`,
);
function measure() {
  if (!viewport.value || !text.value) return;
  overflow.value = Math.max(
    0,
    props.vertical
      ? text.value.scrollHeight - viewport.value.clientHeight
      : text.value.scrollWidth - viewport.value.clientWidth,
  );
}
let observer: ResizeObserver | undefined;
onMounted(() => {
  observer = new ResizeObserver(measure);
  if (viewport.value) observer.observe(viewport.value);
  if (text.value) observer.observe(text.value);
  measure();
});
onUnmounted(() => observer?.disconnect());
watch(
  () => [props.text, props.vertical],
  async () => {
    await nextTick();
    measure();
  },
);
</script>
<template>
  <div
    ref="viewport"
    class="lyric-channel"
    :class="{ vertical, auxiliary }"
    :style="{
      justifyContent:
        overflow > 0 || align === 'left' ? 'flex-start' : align === 'right' ? 'flex-end' : 'center',
    }"
  >
    <p
      ref="text"
      class="desktop-lyric-text"
      :class="auxiliary ? 'desktop-lyric-auxiliary' : 'desktop-lyric-original'"
      :style="{ transform: offset }"
      :aria-label="props.text"
    >
      <template v-if="words?.length"
        ><LyricInk
          v-for="(word, index) in words"
          :key="index"
          :vertical="vertical"
          :text="word.text"
          :progress="word.progress"
      /></template>
      <LyricInk v-else :vertical="vertical" :text="props.text" :progress="active ? 1 : 0" />
    </p>
  </div>
</template>
