<script setup lang="ts">
import { computed, onUnmounted, watch } from 'vue';
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
function pause() {
  if (timer) clearTimeout(timer);
  timer = undefined;
}
function start() {
  pause();
  if (visible.value) timer = setTimeout(() => void props.model.dismiss(), 8000);
}
function leave(event: FocusEvent) {
  if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) start();
}
function close(event: Event) {
  if (
    (event.currentTarget as HTMLElement).closest('.relay-prompt')?.contains(document.activeElement)
  )
    document.querySelector<HTMLButtonElement>('.settings-button')?.focus({ preventScroll: true });
  void props.model.dismiss();
}
watch(visible, start, { immediate: true });
onUnmounted(pause);
</script>
<template>
  <Transition name="relay-prompt">
    <aside
      v-if="visible && model.alert.value"
      class="relay-prompt"
      aria-label="跨端续播"
      @pointerenter="pause"
      @pointerleave="start"
      @focusin="pause"
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
