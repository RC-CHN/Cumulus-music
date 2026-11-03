<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import { useControlFocus } from '../../composables/useControlFocus';
import { penaltyInstruction, penaltyStepLabel } from '../../services/loginPenalty';
import Icon from '../Icon.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ phone: [] }>();
const { view, busy, error } = props.model;
const penalty = computed(() => view.value?.penalty);
const root = ref<HTMLElement>();
let alive = true;
const focus = useControlFocus(
  () => (alive ? view.value?.operationId : null),
  () =>
    root.value?.querySelector<HTMLElement>('.penalty-check:enabled, .penalty-restart') ??
    root.value,
);
function clearSecrets() {
  alive = false;
}
defineExpose({ clearSecrets });
async function check(event: Event) {
  const current = penalty.value;
  if (!alive || busy.value || !current?.canCheckStatus) return;
  await focus.run(event, () =>
    props.model.run({ kind: 'penalty_check', verification_id: current.id }),
  );
}
onMounted(async () => {
  await nextTick();
  if (alive && root.value?.isConnected && !root.value.closest('[inert]')) {
    const focus = document.activeElement;
    if (focus === document.body || root.value.closest('.login-flow')?.contains(focus))
      root.value.focus({ preventScroll: true });
  }
});
onBeforeUnmount(clearSecrets);
</script>
<template>
  <section
    v-if="penalty"
    ref="root"
    class="penalty-verification"
    tabindex="-1"
    aria-label="账号安全确认"
    :aria-busy="busy"
  >
    <button type="button" class="penalty-back" aria-label="返回手机登录" @click="emit('phone')">
      <Icon name="back" />
    </button>
    <Icon name="lock" class="penalty-symbol" />
    <p class="penalty-instruction">{{ penaltyInstruction(penalty) }}</p>
    <ol
      v-if="penalty.steps.length"
      class="penalty-steps"
      tabindex="0"
      aria-label="服务端报告的验证步骤"
    >
      <li
        v-for="(step, index) in penalty.steps"
        :key="index"
        :aria-current="penalty.currentStep === index ? 'step' : undefined"
        :data-passed="step.passed"
      >
        <Icon :name="step.passed ? 'check' : 'lock'" />
        <div>
          <strong>{{ penaltyStepLabel(step) }}</strong>
          <p v-if="step.kind === 'unknown'">
            {{ step.passed ? '此步骤由官方客户端处理' : '当前客户端尚不支持此验证步骤' }}
          </p>
          <small v-if="step.minVersion">官方验证组件要求：{{ step.minVersion }}</small>
        </div>
        <span>{{
          step.passed ? '已完成' : penalty.currentStep === index ? '待处理' : '未完成'
        }}</span>
      </li>
    </ol>
    <p
      v-if="penalty.phase !== 'status_passed' && (penalty.phase === 'read_failed' || error)"
      class="penalty-error"
      role="alert"
    >
      {{ error || '状态暂未取得，请稍后重新核对。' }}
    </p>
    <p v-if="penalty.phase === 'status_passed'" class="penalty-result" role="status">
      当前未确认登录。若你修改过密码，请使用新密码登录。
    </p>
    <button
      v-if="penalty.phase !== 'status_passed'"
      type="button"
      class="primary penalty-check"
      :disabled="busy || !penalty.canCheckStatus"
      @click="check"
    >
      {{
        busy
          ? '正在核对…'
          : penalty.phase === 'read_failed'
            ? '重新核对状态'
            : '已在官方端处理，核对状态'
      }}
    </button>
    <button type="button" class="penalty-restart" @click="emit('phone')">
      {{ penalty.phase === 'status_passed' ? '重新登录' : '返回手机登录' }}
    </button>
  </section>
</template>
