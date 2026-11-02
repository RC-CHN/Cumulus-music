<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import Icon from '../Icon.vue';
const props = defineProps<{ model: LoginModel }>();
const device = computed(() => props.model.view.value?.device);
const code = ref('');
let timer: ReturnType<typeof setTimeout> | undefined;
let alive = true;
function stop() {
  clearTimeout(timer);
  timer = undefined;
}
function clearSecrets() {
  code.value = '';
  stop();
}
defineExpose({ clearSecrets });
watch(
  () => [
    device.value?.id,
    device.value?.phase,
    props.model.busy.value,
    props.model.view.value?.status,
  ],
  () => {
    stop();
    const value = device.value;
    if (
      !alive ||
      !value ||
      props.model.busy.value ||
      props.model.view.value?.status !== 'two_factor' ||
      !['waiting', 'scanned'].includes(value.phase)
    )
      return;
    timer = setTimeout(
      () => {
        if (alive && device.value?.id === value.id)
          void props.model.run({ kind: 'device_poll', verification_id: value.id });
      },
      Math.max(100, Math.min(value.pollAfterMs, 60000)),
    );
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  alive = false;
  clearSecrets();
});
const matrix = computed(() => {
  const modules = device.value?.modules;
  if (!modules) return '';
  return modules
    .flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x} ${y}h1v1h-1z`] : [])))
    .join('');
});
const label = computed(() => {
  const value = device.value;
  if (!value) return '';
  const names: Record<string, string> = {
    starting: '正在申请原设备验证…',
    waiting: value.kind === 'scan' ? '请使用已登录该账号的云音乐 App 扫码' : '请在原设备查看确认码',
    scanned: '扫码成功，请在原设备确认',
    code_required: '请完成原设备确认码验证',
    completed: '此步骤已完成，请继续验证',
    expired: '验证已过期，请选择其他方式或重新申请',
    rejected: '验证未通过，请选择其他验证方式',
    blocked: '原设备已拒绝登录请求，请次日再试或尝试其他验证方式',
    error: '暂时无法读取验证状态，可以重新检查',
    recovery_required: '无法确认上次请求的结果，请先核对登录状态',
    cancelled: '已停止原设备验证',
    stopped: '已停止本机验证',
    stop_unconfirmed: '已停止本机轮询，尚未确认远端取消结果',
  };
  return names[value.phase] ?? '';
});
async function verify() {
  const value = device.value;
  if (!value || props.model.busy.value) return;
  const answer = code.value;
  code.value = '';
  await props.model.run({ kind: 'device_verify', verification_id: value.id, code: answer });
}
</script>
<template>
  <section
    v-if="device"
    class="device-verification"
    :data-device-phase="device.phase"
    :data-device-kind="device.kind"
    :aria-busy="model.busy.value"
  >
    <p v-if="device.deviceName" class="verification-device-name" :title="device.deviceName">
      {{
        device.deviceName.length > 15 ? device.deviceName.slice(0, 15) + '...' : device.deviceName
      }}
    </p>
    <div
      v-if="device.kind === 'scan' && ['waiting', 'scanned'].includes(device.phase)"
      class="device-qr-frame"
    >
      <svg
        v-if="device.phase === 'waiting' && device.modules"
        role="img"
        aria-label="原设备验证二维码"
        :viewBox="`0 0 ${device.modules.length} ${device.modules.length}`"
        shape-rendering="crispEdges"
      >
        <rect
          x="0"
          y="0"
          :width="device.modules.length"
          :height="device.modules.length"
          fill="white"
        />
        <path :d="matrix" fill="black" />
      </svg>
      <Icon v-else name="check" />
    </div>
    <p class="device-status" role="status">{{ label }}</p>
    <p v-if="device.safeDeviceCount === 0" class="muted">
      暂未找到可用的原设备，请选择其他验证方式。
    </p>
    <form
      v-if="device.kind === 'confirm' && ['waiting', 'code_required'].includes(device.phase)"
      class="credential-form"
      @submit.prevent="verify"
    >
      <label
        >原设备确认码<input
          v-model="code"
          aria-label="原设备确认码"
          autocomplete="one-time-code"
          inputmode="numeric"
          pattern="[0-9]{6}"
          maxlength="6"
          required
          :disabled="model.busy.value"
      /></label>
      <p v-if="device.answerRejected" class="verification-error" role="status">
        确认码错误，请重新输入
      </p>
      <button
        class="primary login-submit"
        type="submit"
        :disabled="model.busy.value || !/^[0-9]{6}$/.test(code)"
      >
        确认验证
      </button>
    </form>
    <button
      v-if="device.phase === 'error'"
      :disabled="model.busy.value"
      @click="model.run({ kind: 'device_poll', verification_id: device.id })"
    >
      重新检查验证状态
    </button>
    <button
      v-if="device.phase === 'recovery_required'"
      :disabled="model.busy.value"
      @click="model.run({ kind: 'reconcile' })"
    >
      核对登录状态
    </button>
  </section>
</template>
