<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useWindowLifecycle } from '../composables/useWindowLifecycle';
import type { CloseBehavior } from '../services/window';
import ModalTransition from './ModalTransition.vue';
import CloseDialog from './WindowCloseDialog.vue';
const model = useWindowLifecycle();
const behavior = ref<CloseBehavior>('tray'),
  remember = ref(false);
const prompt = computed(() => model.view.value?.prompt);
const busy = computed(() => model.pending.value || !!model.view.value?.busy);
const dismissed = ref<string>();
watch(
  () => prompt.value?.id,
  () => {
    behavior.value = prompt.value?.preferences.settings.closeBehavior ?? 'tray';
    remember.value = false;
    model.error.value = '';
  },
);
function cancel() {
  if (prompt.value) void model.command({ kind: 'cancel', id: prompt.value.id });
}
function confirm() {
  if (prompt.value)
    void model.command({
      kind: 'confirm',
      id: prompt.value.id,
      behavior: behavior.value,
      remember: remember.value,
    });
}
const fallbackFocus = () => document.querySelector<HTMLButtonElement>('.window-close');
onMounted(model.connect);
onUnmounted(model.dispose);
</script>
<template>
  <ModalTransition :fallback-focus="fallbackFocus">
    <CloseDialog
      v-if="prompt"
      :key="prompt.id"
      v-model:behavior="behavior"
      v-model:remember="remember"
      :busy="busy"
      :available="!!model.view.value?.trayAvailable"
      :damaged="prompt.preferences.damaged"
      :error="model.error.value || model.view.value?.message || ''"
      @cancel="cancel"
      @confirm="confirm"
    />
  </ModalTransition>
  <aside
    v-if="!prompt && model.view.value?.message && dismissed !== model.view.value.message"
    class="window-lifecycle-notice notice"
    role="alert"
  >
    <span>{{ model.view.value.message }}</span
    ><button class="text-button" @click="dismissed = model.view.value.message">知道了</button>
  </aside>
</template>
<style scoped>
.window-lifecycle-notice {
  position: fixed;
  z-index: 90;
  bottom: 110px;
  left: 50%;
  transform: translateX(-50%);
  max-width: min(560px, calc(100vw - 40px));
  box-shadow: var(--shadow);
}
</style>
