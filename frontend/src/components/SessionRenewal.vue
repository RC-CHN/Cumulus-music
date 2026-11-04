<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { useCore } from '../services/core';
import { useControlFocus } from '../composables/useControlFocus';
import { useSessionRenewal } from '../composables/useSessionRenewal';
import { renewalMessage } from '../services/renewal';
const props = defineProps<{ epoch: string; disabled?: boolean }>();
const emit = defineEmits<{ login: [] }>();
const model = useSessionRenewal(useCore(), props.epoch);
const { view, busy, error } = model;
const section = ref<HTMLElement>();
const runButton = ref<HTMLButtonElement>();
const readButton = ref<HTMLButtonElement>();
const loginButton = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => props.epoch,
  () => readButton.value ?? runButton.value ?? loginButton.value ?? section.value,
);
function run(event: MouseEvent) {
  void focus.run(event, model.run);
}
function reload(event: MouseEvent) {
  void focus.run(event, model.load);
}
onMounted(() => void model.connect());
onUnmounted(model.dispose);
</script>
<template>
  <section
    ref="section"
    class="session-renewal"
    aria-label="登录续期"
    :data-state="view?.state"
    tabindex="-1"
  >
    <p aria-live="polite">{{ view ? renewalMessage(view.state) : '正在读取续期状态…' }}</p>
    <small v-if="view?.lastSuccess"
      >上次成功：{{ new Date(view.lastSuccess * 1000).toLocaleString() }}</small
    >
    <button
      v-if="view?.canCheck || view?.canReconcile"
      ref="runButton"
      :disabled="disabled || busy || view.busy"
      @click="run"
    >
      {{ busy ? '正在检查…' : view.canReconcile ? '核对续期结果' : '检查登录续期' }}
    </button>
    <button
      v-if="view?.state === 'unconfirmed'"
      ref="readButton"
      :disabled="disabled || busy"
      @click="reload"
    >
      重新读取续期状态
    </button>
    <button
      v-if="view && ['expired', 'unconfirmed'].includes(view.state)"
      ref="loginButton"
      :disabled="disabled || busy"
      @click="emit('login')"
    >
      重新登录
    </button>
    <p v-if="error" role="alert">
      {{ error
      }}<button v-if="view?.state !== 'unconfirmed'" :disabled="disabled || busy" @click="reload">
        重新读取状态
      </button>
    </p>
  </section>
</template>
<style scoped>
.session-renewal {
  margin: 12px 0;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  font-size: 12px;
}
.session-renewal p {
  margin: 0 0 6px;
  line-height: 1.6;
}
.session-renewal small {
  display: block;
  margin: 4px 0;
  opacity: 0.7;
}
.session-renewal button {
  margin: 4px 10px 0 0;
  font-size: 12px;
}
</style>
