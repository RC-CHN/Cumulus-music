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
let disposed = false;
const ownsFocus = () => !!element.value?.contains(document.activeElement);
function pause() {
  if (timer) clearTimeout(timer);
  timer = undefined;
}
function start() {
  pause();
  if (!disposed && visible.value && !hovered && !ownsFocus())
    timer = setTimeout(() => {
      timer = undefined;
      // A window blur may omit relatedTarget and a WebView can deliver its
      // focus events late. Never dismiss a control that still owns DOM focus.
      if (!disposed && visible.value && !hovered && !ownsFocus()) void props.model.dismiss();
    }, 8000);
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
  pause();
}
function leave() {
  // activeElement is transient while focusout is being dispatched. Observe
  // the completed focus move, including moves between this prompt's buttons.
  queueMicrotask(start);
}
function beforeLeave(node: Element) {
  pause();
  // The transition keeps this DOM alive after its offer has been dismissed.
  // It must stop accepting focus/clicks as soon as the exit begins.
  node.setAttribute('inert', '');
}
function cancelLeave(node: Element) {
  node.removeAttribute('inert');
  hovered = node.matches(':hover');
  start();
}
function continueHere() {
  if (visible.value && props.model.alert.value) void props.model.continueHere(props.quality);
}
function close(event: Event) {
  if (!visible.value || !props.model.alert.value) return;
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
    start();
  },
  { immediate: true, flush: 'post' },
);
onUnmounted(() => {
  disposed = true;
  pause();
});
</script>
<template>
  <Transition name="relay-prompt" @before-leave="beforeLeave" @leave-cancelled="cancelLeave">
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
        @click="continueHere"
      >
        {{ model.alert.value.buttonText }}
      </button>
      <button class="icon-button relay-close" aria-label="关闭接续提示" @click="close">
        <Icon name="close" />
      </button>
    </aside>
  </Transition>
</template>
