<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import Icon from '../Icon.vue';
import PinCode from './PinCode.vue';
const props = defineProps<{ model: LoginModel; kind: 'confirm' | 'scan' }>();
const device = computed(() => props.model.view.value?.device);
const root = ref<HTMLElement>();
const pin = ref<{ clear: () => void; focus: () => void }>();
const pendingAnswer = ref(''),
  confirming = ref(false),
  refreshing = ref(false),
  edited = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;
let alive = true;
function stop() {
  clearTimeout(timer);
  timer = undefined;
}
function clearSecrets() {
  pendingAnswer.value = '';
  pin.value?.clear();
  stop();
}
defineExpose({ clearSecrets });
const acceptsCode = computed(
  () =>
    device.value?.kind === 'confirm' && ['waiting', 'code_required'].includes(device.value.phase),
);
async function sendAnswer() {
  const value = device.value;
  if (
    !alive ||
    !value ||
    !acceptsCode.value ||
    props.model.busy.value ||
    confirming.value ||
    !pendingAnswer.value
  )
    return;
  const answer = pendingAnswer.value;
  pendingAnswer.value = '';
  confirming.value = true;
  edited.value = false;
  stop();
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'device_verify', verification_id: value.id, code: answer });
  confirming.value = false;
  await nextTick();
  if (
    alive &&
    device.value?.id === value.id &&
    acceptsCode.value &&
    document.activeElement === root.value
  )
    pin.value?.focus();
}
function complete(answer: string) {
  if (!alive || confirming.value || pendingAnswer.value || !acceptsCode.value) return;
  pendingAnswer.value = answer;
  pin.value?.clear();
  root.value?.focus({ preventScroll: true });
  void sendAnswer();
}
watch(
  () => [
    device.value?.id,
    device.value?.phase,
    props.model.busy.value,
    props.model.view.value?.status,
    pendingAnswer.value,
    confirming.value,
    refreshing.value,
  ],
  () => {
    stop();
    if (!acceptsCode.value) {
      pendingAnswer.value = '';
      pin.value?.clear();
    }
    if (pendingAnswer.value) {
      void sendAnswer();
      return;
    }
    const value = device.value;
    if (
      !alive ||
      !value ||
      props.model.busy.value ||
      confirming.value ||
      refreshing.value ||
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
watch(
  () => device.value?.id,
  () => {
    pendingAnswer.value = '';
    edited.value = false;
    pin.value?.clear();
  },
);
onBeforeUnmount(() => {
  alive = false;
  clearSecrets();
});
const matrix = computed(
  () =>
    device.value?.modules
      ?.flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x} ${y}h1v1h-1z`] : [])))
      .join('') ?? '',
);
const name = computed(() => {
  const value = device.value?.deviceName ?? '';
  return value.length > 15 ? value.slice(0, 15) + '...' : value;
});
const label = computed(() => {
  if (!device.value || refreshing.value) return '正在申请原设备验证…';
  if (pendingAnswer.value || confirming.value) return '正在校验确认码…';
  const value = device.value;
  const names: Record<string, string> = {
    starting: '正在申请原设备验证…',
    waiting: value.kind === 'scan' ? '请使用已登录该账号的云音乐 App 扫码' : '已发送安全确认',
    scanned: '扫码成功，等待确认',
    code_required: '已发送安全确认',
    completed: '此步骤已完成，请继续验证',
    expired: '验证请求已过期或验证错误次数过多，请重新发送',
    rejected: '验证请求已过期或验证错误次数过多，请重新发送',
    blocked: '原设备已拒绝登录请求，请次日再试或尝试其他验证方式',
    error: '暂时无法读取验证状态，可以重新检查',
    recovery_required: '无法确认上次请求的结果，请先核对登录状态',
    cancelled: '已停止原设备验证',
    stopped: '已停止本机验证',
    stop_unconfirmed: '已停止本机轮询，尚未确认远端取消结果',
  };
  return names[value.phase] ?? '';
});
const scanFrame = computed(
  () =>
    device.value?.kind === 'scan' &&
    ['waiting', 'scanned', 'expired', 'rejected'].includes(device.value.phase),
);
const canRestart = computed(
  () => !!device.value && ['expired', 'rejected'].includes(device.value.phase),
);
async function restart() {
  const value = device.value,
    operation = props.model.view.value?.operationId;
  if (!alive || !value || !canRestart.value || props.model.busy.value || refreshing.value) return;
  refreshing.value = true;
  clearSecrets();
  root.value?.focus({ preventScroll: true });
  try {
    const stopped = await props.model.stopDevice(value.id);
    if (
      !alive ||
      props.model.view.value?.operationId !== operation ||
      stopped?.device?.id !== value.id ||
      !['stopped', 'cancelled', 'stop_unconfirmed'].includes(stopped.device.phase)
    )
      return;
    await props.model.run({ kind: 'device_start', method: props.kind });
  } finally {
    refreshing.value = false;
    await nextTick();
    if (alive && root.value?.isConnected && !root.value.closest('[inert]')) {
      if (acceptsCode.value) pin.value?.focus();
      else root.value.focus({ preventScroll: true });
    }
  }
}
</script>
<template>
  <section
    ref="root"
    class="device-verification"
    tabindex="-1"
    :data-device-phase="device?.phase ?? 'starting'"
    :data-device-kind="kind"
    :aria-busy="model.busy.value || refreshing"
  >
    <template v-if="!device || refreshing">
      <p class="device-status device-loading" role="status">
        <span class="device-spinner" aria-hidden="true" /><span class="device-loading-text"
          >正在申请原设备验证…</span
        >
      </p>
    </template>
    <template v-else>
      <p v-if="acceptsCode" class="device-status" role="status">{{ label }}</p>
      <p
        v-if="device.deviceName && acceptsCode"
        class="verification-device-name"
        :title="device.deviceName"
      >
        请打开 <strong>{{ name }}</strong> 获取登录验证码
      </p>
      <p v-if="scanFrame" class="device-status" :title="device.deviceName ?? undefined">
        请使用已登录该账号的 {{ name
        }}{{ (device.safeDeviceCount ?? 0) > 1 ? ' 等' : '' }} 设备扫码验证
      </p>
      <div v-if="scanFrame" class="device-qr-frame">
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
        <div v-else class="device-qr-mask" role="status">
          <template v-if="device.phase === 'scanned'"
            ><Icon name="check" /><span>扫码成功</span><span>等待确认</span></template
          >
          <button
            v-else
            class="device-refresh"
            :disabled="model.busy.value"
            aria-label="刷新原设备二维码"
            @click="restart"
          >
            <Icon name="refresh" /><span>二维码失效</span><span>点击刷新</span>
          </button>
        </div>
      </div>
      <p v-if="!acceptsCode && !scanFrame" class="device-status" role="status">{{ label }}</p>
      <p v-if="device.safeDeviceCount === 0" class="muted">
        暂未找到可用的原设备，请选择其他验证方式。
      </p>
      <div v-if="acceptsCode" class="device-confirm-content">
        <p>输入验证码以继续登录</p>
        <PinCode
          ref="pin"
          :disabled="confirming || !!pendingAnswer"
          @complete="complete"
          @edit="edited = true"
        />
        <p class="verification-error" role="status">
          {{ device.answerRejected && !edited && !confirming ? '确认码错误，请重新输入' : '' }}
        </p>
        <div class="device-instructions">
          <p>
            1、请在 5 分钟内使用已登录 {{ name }} 等设备打开最新版本
            <strong>网易云音乐 App</strong>，超时请重试。
          </p>
          <p>2、在确认页面点击“允许登录”后，获取登录验证码。</p>
        </div>
      </div>
      <div v-if="scanFrame" class="device-instructions">
        <p>1、打开最新版本 <strong>网易云音乐 App</strong></p>
        <p>2、点击首页左上角的侧边栏</p>
        <p>3、点击扫一扫，扫描以上二维码</p>
      </div>
      <button
        v-if="canRestart && !scanFrame"
        class="primary device-resend"
        :disabled="model.busy.value"
        @click="restart"
      >
        重新发送
      </button>
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
    </template>
  </section>
</template>
