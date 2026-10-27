<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import type { MvModel } from '../composables/useMv';
const props = defineProps<{ model: MvModel }>();
const canvas = ref<HTMLCanvasElement>();
const error = ref('');
let stopped = false,
  sequence = 0n,
  key = '',
  rendered = 0,
  last = 0;
async function draw() {
  if (stopped) return;
  const el = canvas.value,
    state = props.model.video.value,
    epoch = props.model.epoch.value;
  const current = `${epoch}:${state?.playId ?? ''}`;
  if (current !== key) {
    key = current;
    sequence = 0n;
    rendered = 0;
    error.value = '';
    el?.getContext('2d')?.clearRect(0, 0, el.width, el.height);
  }
  if (
    el &&
    epoch &&
    state?.playId &&
    ['playing', 'paused', 'ended'].includes(state.state) &&
    performance.now() - last >= 30
  ) {
    last = performance.now();
    const box = el.getBoundingClientRect();
    const scale = Math.min(window.devicePixelRatio || 1, 1920 / box.width, 1080 / box.height);
    const width = Math.max(16, Math.min(1920, Math.round(box.width * scale))),
      height = Math.max(16, Math.min(1080, Math.round(box.height * scale)));
    try {
      const frame = await props.model.frame(
        epoch,
        state.playId,
        sequence.toString(),
        width,
        height,
      );
      if (
        !stopped &&
        current === `${props.model.epoch.value}:${props.model.video.value?.playId ?? ''}` &&
        props.model.video.value?.state !== 'idle' &&
        frame &&
        frame.sequence > sequence
      ) {
        if (el.width !== frame.width) el.width = frame.width;
        if (el.height !== frame.height) el.height = frame.height;
        el.getContext('2d')?.putImageData(
          new ImageData(frame.pixels, frame.width, frame.height),
          0,
          0,
        );
        sequence = frame.sequence;
        rendered++;
        el.dataset.frameSequence = sequence.toString();
        el.dataset.renderedFrames = String(rendered);
        error.value = '';
      }
    } catch {
      if (!stopped && current === key) error.value = '画面暂时无法读取，请重新加载视频';
    }
  }
  if (!stopped) requestAnimationFrame(() => void draw());
}
watch(
  [
    () => props.model.epoch.value,
    () => props.model.video.value?.playId,
    () => props.model.video.value?.state === 'idle',
  ],
  () => {
    canvas.value?.getContext('2d')?.clearRect(0, 0, canvas.value.width, canvas.value.height);
  },
);
onMounted(() => void draw());
onUnmounted(() => {
  stopped = true;
});
</script>
<template>
  <canvas ref="canvas" class="video-canvas" aria-label="MV 视频画面" /><span
    v-if="error"
    class="video-message"
    role="alert"
    >{{ error }}</span
  >
</template>
