<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import { usePlaybackVolume, type VolumeControls } from '../composables/usePlaybackVolume';
import Icon from './Icon.vue';
import MenuTransition from './MenuTransition.vue';
import type { VolumeDiagnosisVisit } from '../services/systemMasterVolume';
const props = defineProps<{ player: VolumeControls; mini?: boolean }>();
const emit = defineEmits<{ diagnose: [visit: VolumeDiagnosisVisit] }>();
const model = usePlaybackVolume(props.player);
const buttonLabel = computed(() =>
  props.mini ? '音量调节' : model.value.value === 0 ? '取消静音' : '静音',
);
const opened = ref(false),
  root = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>(),
  retryButton = ref<HTMLButtonElement>(),
  slider = ref<HTMLInputElement>();
const id = useId();
let hide: ReturnType<typeof setTimeout> | undefined;
let visit: { epoch: string; before: number; revision: number } | undefined;
let generation = 0;
let restoringFocus = false;
function focusTrigger() {
  restoringFocus = true;
  try {
    trigger.value?.focus({ preventScroll: true });
  } finally {
    restoringFocus = false;
  }
}
defineExpose({ focusTrigger });
function show() {
  if (!props.player.ready.value) return;
  clearTimeout(hide);
  if (opened.value) return;
  generation++;
  const epoch = props.player.view.value?.epoch;
  if (!props.mini && epoch !== undefined)
    visit = { epoch, before: model.value.value, revision: model.inputRevision() };
  opened.value = true;
}
function close(restore = false, diagnose = true) {
  clearTimeout(hide);
  const current = visit;
  visit = undefined;
  const after = model.value.value,
    revision = model.inputRevision(),
    serial = generation;
  opened.value = false;
  model.end();
  if (restore) focusTrigger();
  if (
    !diagnose ||
    !current ||
    current.revision === revision ||
    !(after >= 80 && after > current.before)
  )
    return;
  void model.settled().then(() => {
    if (
      serial === generation &&
      revision === model.inputRevision() &&
      props.player.ready.value &&
      props.player.view.value?.epoch === current.epoch &&
      !model.issue.value &&
      model.native.value === after
    )
      emit('diagnose', { epoch: current.epoch, before: current.before, after });
  });
}
function leave() {
  clearTimeout(hide);
  hide = setTimeout(() => {
    if (!root.value?.contains(document.activeElement) && !model.dragging.value) close();
  }, 120);
}
function hover() {
  if (!props.mini) show();
}
function pointerLeave() {
  if (!props.mini) leave();
}
function focus(event: FocusEvent) {
  if (restoringFocus || props.mini || root.value?.contains(event.relatedTarget as Node)) return;
  show();
}
function keydown(event: KeyboardEvent) {
  if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;
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
function clickTrigger() {
  if (!props.mini) model.toggleMute();
  else if (opened.value) close(true);
  else show();
}
function wheel(event: WheelEvent) {
  if (!opened.value || !props.player.ready.value) return;
  event.preventDefault();
  model.wheel(event.deltaY);
}
function wheelWhileOpen(event: WheelEvent) {
  if (event.deltaY !== 0 && Number.isFinite(event.deltaY)) wheel(event);
}
watch(
  [opened, () => props.mini],
  ([visible, mini], _old, onCleanup) => {
    if (!visible || mini) return;
    document.addEventListener('wheel', wheelWhileOpen, { passive: false });
    onCleanup(() => document.removeEventListener('wheel', wheelWhileOpen));
  },
  { flush: 'sync' },
);
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
watch(
  [() => props.player.view.value?.epoch, props.player.ready, () => props.player.scope?.value],
  () => {
    generation++;
    close(false, false);
  },
  { flush: 'sync' },
);
onMounted(() => document.addEventListener('pointerdown', outside, true));
onUnmounted(() => {
  generation++;
  clearTimeout(hide);
  document.removeEventListener('pointerdown', outside, true);
});
</script>
<template>
  <div
    ref="root"
    class="volume-control"
    @pointerenter="hover"
    @pointerleave="pointerLeave"
    @focusin="focus"
    @focusout="leave"
    @keydown="keydown"
    @wheel="mini && wheel($event)"
  >
    <button
      ref="trigger"
      class="icon-button volume-trigger"
      :aria-label="buttonLabel"
      :title="
        mini ? `音量 ${model.percent.value}%` : `${buttonLabel} · 音量 ${model.percent.value}%`
      "
      :aria-controls="opened ? id : undefined"
      :aria-expanded="opened"
      :disabled="!player.ready.value"
      @click="clickTrigger"
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
            :aria-valuetext="`${model.percent.value}%`"
            type="range"
            min="0"
            max="100"
            step="1"
            :value="model.percent.value"
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
        <output class="volume-percentage">{{ model.percent.value }}%</output>
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
