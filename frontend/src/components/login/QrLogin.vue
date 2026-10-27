<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useCore } from '../../services/core';
import { failure, type QrView, type QrStatus } from '../../services/contracts';
const emit = defineEmits<{ authorized: [] }>();
const core = useCore();
const qr = ref<QrView>();
const status = ref<QrStatus>('creating');
const error = ref('');
const captchaBusy = ref(false);
let generation = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let disposed = false;
const path = computed(
  () =>
    qr.value?.modules
      .flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x + 4} ${y + 4}h1v1h-1z`] : [])))
      .join(' ') ?? '',
);
const size = computed(() => (qr.value?.modules.length ?? 21) + 8);
const labels: Record<QrStatus, string> = {
  idle: '扫码登录',
  creating: '正在生成二维码…',
  waiting_scan: '使用网易云音乐 App 扫码登录',
  waiting_confirmation: '扫码成功，请在手机上确认',
  authorized: '登录成功',
  expired: '二维码已过期',
  challenge: '请完成安全验证后继续扫码登录',
  cancelled: '已取消',
  error: '暂时无法登录',
};
async function cancel(id: string) {
  try {
    await core.cancelQr(id);
  } catch {
    /* Operation can already be terminal. */
  }
}
async function begin() {
  const current = ++generation;
  clearTimeout(timer);
  error.value = '';
  status.value = 'creating';
  const old = qr.value;
  qr.value = undefined;
  if (old) await cancel(old.operationId);
  if (disposed || current !== generation) return;
  try {
    const value = await core.beginQr();
    if (disposed || current !== generation) {
      await cancel(value.operationId);
      return;
    }
    qr.value = value;
    status.value = value.status;
    timer = setTimeout(() => poll(current), 2000);
  } catch (e) {
    if (!disposed && current === generation) {
      error.value = failure(e).message;
      status.value = 'error';
    }
  }
}
async function poll(current: number) {
  if (disposed || current !== generation || !qr.value) return;
  try {
    const next = await core.pollQr(qr.value.operationId);
    if (disposed || current !== generation) return;
    status.value = next;
    if (next === 'authorized') {
      emit('authorized');
      return;
    }
    if (next === 'waiting_scan' || next === 'waiting_confirmation')
      timer = setTimeout(() => poll(current), 2000);
  } catch (e) {
    if (!disposed && current === generation) {
      error.value = failure(e).message;
      status.value = 'error';
    }
  }
}
async function captcha() {
  if (!qr.value || captchaBusy.value || status.value !== 'challenge') return;
  const current = generation;
  captchaBusy.value = true;
  error.value = '';
  try {
    const next = await core.solveLoginCaptcha(qr.value.operationId);
    if (disposed || current !== generation) return;
    if (next.kind !== 'qr') throw new Error('Unexpected login state');
    status.value = next.value;
    if (next.value === 'authorized') emit('authorized');
    else if (next.value === 'waiting_scan' || next.value === 'waiting_confirmation')
      timer = setTimeout(() => poll(current), 2000);
  } catch (e) {
    if (!disposed && current === generation) error.value = failure(e).message;
  } finally {
    captchaBusy.value = false;
  }
}
onMounted(() => {
  void begin();
});
onUnmounted(() => {
  disposed = true;
  generation++;
  clearTimeout(timer);
  if (qr.value) void cancel(qr.value.operationId);
});
</script>
<template>
  <div class="qr-frame" :aria-busy="status === 'creating'">
    <svg
      v-if="qr && !['expired', 'error', 'challenge'].includes(status)"
      :viewBox="`0 0 ${size} ${size}`"
      role="img"
      aria-label="网易云音乐登录二维码"
      shape-rendering="crispEdges"
    >
      <rect :width="size" :height="size" fill="white" />
      <path :d="path" fill="#111" />
    </svg>
    <span v-else-if="status === 'creating'" class="spinner" />
    <button v-else class="primary" @click="begin">重新生成二维码</button>
  </div>
  <button v-if="status === 'challenge'" class="primary" :disabled="captchaBusy" @click="captcha">
    {{ captchaBusy ? '请在验证窗口中完成操作…' : '打开安全验证' }}
  </button>
  <p class="login-status" role="status">{{ error || labels[status] }}</p>
</template>
