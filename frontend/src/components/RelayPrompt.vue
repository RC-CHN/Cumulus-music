<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { RelayModel } from '../composables/useRelay';
import type { Quality } from '../services/player';
import Icon from './Icon.vue';
import { modalDepth } from '../services/modal';
import '../styles/relay.css';
const props = defineProps<{ model: RelayModel; blocked: boolean; quality: Quality }>();
const visible = computed(
  () => !!props.model.alert.value && !props.blocked && !modalDepth.value && !props.model.busy.value,
);
let timer: ReturnType<typeof setTimeout> | undefined;
const element = ref<HTMLElement>();
let hovered = false;
let focused = false;
function pause() {
  if (timer) clearTimeout(timer);
  timer = undefined;
}
function start() {
  pause();
  if (visible.value && !hovered && !focused)
    timer = setTimeout(() => void props.model.dismiss(), 8000);
}
function enter() {
  hovered = true;
  pause();
}
function exit() {
  hovered = false;
  start();
}
function focus() {
  focused = true;
  pause();
}
function leave(event: FocusEvent) {
  focused = (event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null);
  start();
}
function close(event: Event) {
  if (
    (event.currentTarget as HTMLElement).closest('.relay-prompt')?.contains(document.activeElement)
  )
    document.querySelector<HTMLButtonElement>('.settings-button')?.focus({ preventScroll: true });
  void props.model.dismiss();
}
watch(
  visible,
  () => {
    hovered = !!element.value?.matches(':hover');
    focused = !!element.value?.contains(document.activeElement);
    start();
  },
  { immediate: true, flush: 'post' },
);
onUnmounted(pause);
</script>
<template>
  <Transition name="relay-prompt">
    <aside
      ref="element"
      v-if="visible && model.alert.value"
      class="relay-prompt"
      aria-label="跨端续播"
      @pointerenter="enter"
      @pointerleave="exit"
      @focusin="focus"
      @focusout="leave"
    >
      <div class="relay-device"><Icon name="monitor" /></div>
      <div class="relay-prompt-copy" role="status">
        <strong>{{ model.alert.value.title }}</strong
        ><span :title="`${model.alert.value.deviceName} · ${model.alert.value.resourceName}`"
          >上次播放 {{ model.alert.value.resourceName }}</span
        >
      </div>
      <button
        class="primary relay-continue"
        :disabled="!!model.error.value || model.loading.value || !!model.unknown.value"
        @click="model.continueHere(quality)"
      >
        {{ model.alert.value.buttonText }}
      </button>
      <button class="icon-button relay-close" aria-label="关闭接续提示" @click="close">
        <Icon name="close" />
      </button>
    </aside>
  </Transition>
</template>
