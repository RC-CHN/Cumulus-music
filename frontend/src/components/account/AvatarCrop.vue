<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import {
  coverCrop,
  coverPosition,
  coverViewport,
  coverZoomLimits,
  dragCover,
  restoreCoverPosition,
  type CropPosition,
} from '../../services/coverGeometry';
import type { CoverCrop, CoverImage } from '../../services/playlistCover';

const props = defineProps<{ image: CoverImage; disabled: boolean; initial?: CoverCrop | null }>();
const crop = defineModel<CoverCrop>();
const viewport = ref<HTMLElement>();
const factor = ref(1);
const position = ref<CropPosition>({ x: 0, y: 0, zoom: 1 });
let pointer: { id: number; x: number; y: number } | undefined;
let resize: ResizeObserver | undefined;
const limits = computed(() => coverZoomLimits(props.image.width, props.image.height));
const renderedSize = computed(() => coverViewport.size * factor.value);
const imageStyle = computed(() => {
  const scale = (coverViewport.width / props.image.width) * position.value.zoom * factor.value;
  return {
    width: `${props.image.width * scale}px`,
    height: `${props.image.height * scale}px`,
    transform: `translate(${-position.value.x * scale}px,${-position.value.y * scale}px)`,
  };
});
watch(
  () => props.image,
  ({ width, height }) => {
    pointer = undefined;
    position.value = props.initial
      ? restoreCoverPosition(width, height, props.initial)
      : coverPosition(width, height);
  },
  { immediate: true },
);
watch(
  position,
  (value) => {
    crop.value = coverCrop(props.image.width, props.image.height, value);
  },
  { immediate: true, flush: 'sync' },
);
onMounted(() => {
  if (!viewport.value) return;
  factor.value = (viewport.value.clientWidth || coverViewport.width) / coverViewport.width;
  resize = new ResizeObserver(([entry]) => {
    if (entry && entry.contentRect.width > 0)
      factor.value = entry.contentRect.width / coverViewport.width;
  });
  resize.observe(viewport.value);
});
onUnmounted(() => resize?.disconnect());
function zoom(value: number) {
  if (!props.disabled)
    position.value = coverPosition(props.image.width, props.image.height, {
      ...position.value,
      zoom: value,
    });
}
function move(dx: number, dy: number) {
  if (!props.disabled)
    position.value = dragCover(
      props.image.width,
      props.image.height,
      position.value,
      dx / factor.value,
      dy / factor.value,
    );
}
function down(event: PointerEvent) {
  if (props.disabled || event.button !== 0 || pointer) return;
  pointer = { id: event.pointerId, x: event.clientX, y: event.clientY };
  viewport.value?.focus({ preventScroll: true });
  try {
    viewport.value?.setPointerCapture(event.pointerId);
  } catch {
    pointer = undefined;
  }
}
function drag(event: PointerEvent) {
  if (pointer?.id !== event.pointerId) return;
  move(event.clientX - pointer.x, event.clientY - pointer.y);
  pointer = { id: event.pointerId, x: event.clientX, y: event.clientY };
}
function release(event: PointerEvent) {
  if (pointer?.id === event.pointerId) pointer = undefined;
}
function key(event: KeyboardEvent) {
  if (props.disabled) return;
  const step = event.shiftKey ? 20 : 2;
  switch (event.key) {
    case 'ArrowLeft':
      move(-step, 0);
      break;
    case 'ArrowRight':
      move(step, 0);
      break;
    case 'ArrowUp':
      move(0, -step);
      break;
    case 'ArrowDown':
      move(0, step);
      break;
    case '+':
    case '=':
      zoom(position.value.zoom + 0.1);
      break;
    case '-':
      zoom(position.value.zoom - 0.1);
      break;
    default:
      return;
  }
  event.preventDefault();
}
</script>

<template>
  <div
    ref="viewport"
    class="avatar-crop-viewport"
    data-testid="account-avatar-crop"
    role="group"
    :tabindex="disabled ? -1 : 0"
    :aria-disabled="disabled"
    aria-label="头像裁剪区域，方向键移动图片，加减键缩放"
    :data-crop="JSON.stringify(crop)"
    @pointerdown="down"
    @pointermove="drag"
    @pointerup="release"
    @pointercancel="release"
    @lostpointercapture="release"
    @keydown="key"
    @wheel.prevent="zoom(position.zoom * Math.exp(-$event.deltaY * 0.001))"
  >
    <img :src="image.preview" :style="imageStyle" alt="所选头像" draggable="false" />
    <div
      class="avatar-crop-mask"
      :style="{ width: `${renderedSize}px`, height: `${renderedSize}px` }"
      aria-hidden="true"
    ></div>
  </div>
  <input
    class="avatar-crop-zoom"
    type="range"
    data-testid="account-avatar-zoom"
    aria-label="头像缩放"
    :min="limits.min"
    :max="limits.max"
    step="0.01"
    :value="position.zoom"
    :aria-valuetext="`${position.zoom.toFixed(2)} 倍`"
    :disabled="disabled"
    @input="zoom(Number(($event.target as HTMLInputElement).value))"
  />
</template>

<style scoped>
.avatar-crop-viewport {
  position: relative;
  width: 362px;
  max-width: 100%;
  aspect-ratio: 362 / 260;
  overflow: hidden;
  border-radius: 10px;
  background: var(--surface-hover);
  touch-action: none;
  outline-offset: -2px;
}
.avatar-crop-viewport:not([aria-disabled='true']) {
  cursor: grab;
}
.avatar-crop-viewport:not([aria-disabled='true']):active {
  cursor: grabbing;
}
.avatar-crop-viewport img,
.avatar-crop-mask {
  position: absolute;
  top: 50%;
  left: 50%;
  pointer-events: none;
}
.avatar-crop-viewport img {
  max-width: none;
  max-height: none;
  user-select: none;
}
.avatar-crop-mask {
  box-sizing: border-box;
  border: 1px solid #ffffffb0;
  border-radius: 50%;
  box-shadow: 0 0 0 500px #0008;
  transform: translate(-50%, -50%);
}
.avatar-crop-zoom {
  display: block;
  width: 100%;
  margin: 18px 0 0;
  accent-color: var(--accent);
}
</style>
