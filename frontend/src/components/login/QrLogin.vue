<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useCore } from '../../services/core';
import { failure, type QrView, type QrStatus } from '../../services/contracts';
import Icon from '../Icon.vue';
const emit = defineEmits<{ authorized: [] }>();
const core = useCore();
const qr = ref<QrView>();
const status = ref<QrStatus>('creating');
const error = ref('');
const documentError = ref('');
const openingDocument = ref(false);
const captchaBusy = ref(false);
let generation = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let disposed = false;
const frame = ref<HTMLElement>();
const downloadButton = ref<HTMLButtonElement>();
function deactivate() {
  if (disposed) return;
  disposed = true;
  generation++;
  clearTimeout(timer);
  if (qr.value) void cancel(qr.value.operationId);
  qr.value = undefined;
}
defineExpose({ deactivate });
watch(status, async () => {
  const active = document.activeElement;
  const owned = frame.value?.contains(active) || active === downloadButton.value;
  await nextTick();
  if (!disposed && owned && document.activeElement === document.body)
    frame.value?.focus({ preventScroll: true });
});
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
  waiting_scan: '',
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
  captchaBusy.value = false;
  clearTimeout(timer);
  error.value = '';
  documentError.value = '';
  openingDocument.value = false;
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
    timer = setTimeout(() => poll(current), value.pollAfterMs ?? 2000);
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
      timer = setTimeout(() => poll(current), qr.value?.pollAfterMs ?? 2000);
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
      timer = setTimeout(() => poll(current), qr.value?.pollAfterMs ?? 2000);
  } catch (e) {
    if (!disposed && current === generation) error.value = failure(e).message;
  } finally {
    if (!disposed && current === generation) captchaBusy.value = false;
  }
}
async function openDownload() {
  if (disposed || openingDocument.value || status.value !== 'waiting_scan') return;
  const current = generation;
  documentError.value = '';
  openingDocument.value = true;
  try {
    await core.openLoginDocument('app_download');
  } catch {
    if (!disposed && current === generation && status.value === 'waiting_scan')
      documentError.value = '浏览器未能打开，请检查默认浏览器后重试';
  } finally {
    if (!disposed && current === generation) openingDocument.value = false;
  }
}
onMounted(() => {
  void begin();
});
onBeforeUnmount(deactivate);
</script>
<template>
  <div ref="frame" class="qr-frame" tabindex="-1" :aria-busy="status === 'creating'">
    <div
      v-if="status === 'waiting_confirmation'"
      class="qr-confirmation"
      aria-label="扫码成功"
      role="status"
    >
      <Icon name="check" />
      <strong>扫码成功</strong>
      <span>请在手机上确认登录</span>
    </div>
    <svg
      v-else-if="qr && ['waiting_scan', 'authorized'].includes(status)"
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
  <p v-if="status === 'waiting_scan'" class="login-status qr-scan-instructions">
    使用
    <button
      ref="downloadButton"
      type="button"
      class="qr-app-download"
      aria-label="下载网易云音乐 App"
      :aria-disabled="openingDocument"
      @click="openDownload"
    >
      网易云音乐 App
    </button>
    扫码登录
  </p>
  <p v-else class="login-status" role="status">
    {{ error || (status === 'waiting_confirmation' ? '' : labels[status]) }}
  </p>
  <p v-if="status === 'waiting_scan' && documentError" class="login-status" role="alert">
    {{ documentError }}
  </p>
</template>
