<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import { useControlFocus } from '../../composables/useControlFocus';
import { createSecurityQrPoller, securityQrActions } from '../../services/securityQr';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ phone: [] }>();
const core = useCore();
const { view, busy, error } = props.model;
const qr = computed(() => view.value?.securityQr);
const status = computed(() => qr.value?.status ?? 'error');
const root = ref<HTMLElement>();
const frame = ref<HTMLElement>();
const refreshing = ref(false),
  checking = ref(false),
  pollingFailed = ref(false),
  opening = ref(false),
  documentError = ref('');
let alive = true,
  mounted = false,
  actionGeneration = 0;
let checkTimer: ReturnType<typeof setTimeout> | undefined;
function clearCheckTimer() {
  clearTimeout(checkTimer);
  checkTimer = undefined;
  checking.value = false;
}
const owner = computed(() => JSON.stringify([view.value?.operationId, qr.value?.id]));
const documentFocus = useControlFocus(
  () => (alive ? view.value?.operationId : null),
  () => frame.value,
);
const poller = createSecurityQrPoller({
  read: () => {
    const current = qr.value;
    if (!current || !view.value) return null;
    return {
      owner: owner.value,
      verificationId: current.id,
      status: current.status,
      loginStatus: view.value.status,
      pollAfterMs: current.pollAfterMs,
      busy: busy.value || refreshing.value,
      active: alive && mounted && !!root.value?.isConnected && !root.value.closest('[inert]'),
      canReconcile: canReconcile.value,
    };
  },
  poll: async (id) => {
    const next = await props.model.run({ kind: 'security_qr_poll', verification_id: id });
    return !!next && !error.value;
  },
  failed: () => {
    pollingFailed.value = true;
  },
  initialRefresh: () => void refresh(),
});
function deactivate() {
  if (!alive) return;
  alive = false;
  actionGeneration++;
  poller.dispose();
  clearCheckTimer();
  if (root.value) root.value.inert = true;
}
defineExpose({ clearSecrets: deactivate });
function phone() {
  deactivate();
  emit('phone');
}
const matrix = computed(
  () =>
    qr.value?.modules
      ?.flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x + 4} ${y + 4}h1v1h-1z`] : [])))
      .join('') ?? '',
);
const matrixSize = computed(() => (qr.value?.modules?.length ?? 21) + 8);
const actions = computed(() =>
  securityQrActions(qr.value, view.value?.status, pollingFailed.value),
);
const canReconcile = computed(() => actions.value.canReconcile);
const canRefresh = computed(() => actions.value.canRefresh);
async function restoreOwnedFocus(owned: boolean, serial: number) {
  await nextTick();
  if (
    !owned ||
    !alive ||
    serial !== actionGeneration ||
    !root.value?.isConnected ||
    root.value.closest('[inert]')
  )
    return;
  const focused = document.activeElement as HTMLElement | null;
  if (
    focused === document.body ||
    !focused?.isConnected ||
    (root.value.contains(focused) && focused.matches(':disabled'))
  )
    frame.value?.focus({ preventScroll: true });
}
async function refresh(event?: Event) {
  const current = qr.value;
  if (!alive || !current || busy.value || refreshing.value || !canRefresh.value) return;
  const serial = ++actionGeneration;
  const owned = !!root.value?.contains(document.activeElement);
  const operationId = view.value?.operationId;
  poller.pause();
  refreshing.value = true;
  pollingFailed.value = false;
  try {
    const next = await props.model.run({
      kind: 'security_qr_refresh',
      verification_id: current.id,
    });
    if (!alive || serial !== actionGeneration || view.value?.operationId !== operationId) return;
    if (!next || error.value) pollingFailed.value = true;
  } finally {
    if (alive && serial === actionGeneration && view.value?.operationId === operationId) {
      refreshing.value = false;
      if (!pollingFailed.value) poller.resume();
      await restoreOwnedFocus(owned || !!event, serial);
    }
  }
}
async function captcha() {
  if (
    !alive ||
    busy.value ||
    canReconcile.value ||
    status.value !== 'challenge' ||
    !view.value?.captchaAvailable
  )
    return;
  const serial = ++actionGeneration;
  const owned = !!root.value?.contains(document.activeElement);
  const expectedOwner = owner.value;
  poller.pause();
  pollingFailed.value = false;
  const next = await props.model.captcha();
  if (!alive || serial !== actionGeneration || owner.value !== expectedOwner) return;
  if (!next || error.value) pollingFailed.value = true;
  else poller.resume();
  await restoreOwnedFocus(owned, serial);
}
async function reconcile() {
  if (!alive || busy.value || !canReconcile.value) return;
  const serial = ++actionGeneration;
  const owned = !!root.value?.contains(document.activeElement);
  poller.pause();
  await props.model.run({ kind: 'reconcile' });
  await restoreOwnedFocus(owned, serial);
}
async function openDownload(event: Event) {
  if (!alive || opening.value) return;
  const restoreFocus = documentFocus.capture(event.currentTarget);
  opening.value = true;
  documentError.value = '';
  try {
    await core.openLoginDocument('app_download');
  } catch {
    if (alive) documentError.value = '浏览器未能打开，请检查默认浏览器后重试';
  } finally {
    if (alive) opening.value = false;
    await nextTick();
    if (alive && !root.value?.closest('[inert]')) restoreFocus();
  }
}
watch(
  () => [
    owner.value,
    status.value,
    busy.value,
    refreshing.value,
    view.value?.status,
    canReconcile.value,
  ],
  () => {
    poller.sync();
  },
);
watch(owner, () => {
  pollingFailed.value = false;
});
watch(
  () => [owner.value, busy.value, status.value, refreshing.value],
  () => {
    clearCheckTimer();
    // Ordinary polls finish without replacing the QR. A slow verification
    // (including the native identity read after 803) gets explicit feedback.
    if (
      alive &&
      busy.value &&
      !refreshing.value &&
      (status.value === 'waiting_scan' || status.value === 'waiting_confirmation')
    )
      checkTimer = setTimeout(() => {
        if (alive) checking.value = true;
      }, 500);
  },
);
watch(
  () => [status.value, canReconcile.value, canRefresh.value, checking.value],
  async () => {
    const owned = !!root.value?.contains(document.activeElement);
    await restoreOwnedFocus(owned, actionGeneration);
  },
);
onMounted(async () => {
  mounted = true;
  poller.sync();
  await nextTick();
  const flow = root.value?.closest('.login-flow');
  if (
    alive &&
    !root.value?.closest('[inert]') &&
    (document.activeElement === document.body || flow?.contains(document.activeElement))
  )
    frame.value?.focus({ preventScroll: true });
});
onBeforeUnmount(deactivate);
</script>
<template>
  <section
    ref="root"
    class="security-qr-verification"
    aria-label="设备验证"
    :data-security-qr-status="status"
  >
    <button type="button" class="security-qr-back" aria-label="返回手机登录" @click="phone">
      <Icon name="back" />
    </button>
    <p
      v-if="!canReconcile && !checking && status !== 'waiting_confirmation'"
      class="security-qr-description"
    >
      当前登录存在安全风险，为保护账号安全<br />
      请使用 <strong>{{ qr?.nickname || '***' }}</strong> 账号登录的设备扫码验证
    </p>
    <div
      ref="frame"
      class="security-qr-frame"
      tabindex="-1"
      role="group"
      aria-label="设备安全验证"
      :aria-busy="
        refreshing || checking || status === 'creating' || (busy && status === 'authorized')
      "
    >
      <div v-if="canReconcile" class="security-qr-overlay" role="status">
        <span>登录结果尚未确认，请先核对登录状态</span>
        <button type="button" class="primary" :disabled="busy" @click="reconcile">
          {{ busy ? '正在核对…' : '核对登录状态' }}
        </button>
      </div>
      <div v-else-if="checking" class="security-qr-overlay security-qr-checking" role="status">
        <span class="device-spinner" aria-hidden="true" />
        <span>正在核对设备验证结果…</span>
      </div>
      <div
        v-else-if="refreshing || status === 'creating'"
        class="security-qr-overlay"
        role="status"
      >
        <span class="device-spinner" aria-hidden="true" /><span>正在生成验证二维码…</span>
      </div>
      <div
        v-else-if="
          status !== 'authorized' &&
          (view?.status === 'error' ||
            (pollingFailed && ['waiting_scan', 'waiting_confirmation'].includes(status)))
        "
        class="security-qr-overlay"
        role="status"
      >
        <span>暂时无法取得设备验证信息，已停止自动检查</span>
        <button type="button" class="primary" :disabled="busy || !canRefresh" @click="refresh">
          点击重试
        </button>
      </div>
      <div
        v-else-if="status === 'waiting_confirmation'"
        class="security-qr-confirmation"
        role="status"
      >
        <Icon name="check" /><strong>扫码成功</strong><span>请在手机上确认验证</span>
      </div>
      <svg
        v-else-if="status === 'waiting_scan' && qr?.modules"
        :viewBox="`0 0 ${matrixSize} ${matrixSize}`"
        role="img"
        aria-label="设备安全验证二维码"
        shape-rendering="crispEdges"
      >
        <rect :width="matrixSize" :height="matrixSize" fill="white" />
        <path :d="matrix" fill="#111" />
      </svg>
      <div v-else-if="status === 'challenge'" class="security-qr-overlay" role="status">
        <span>请完成安全验证后继续</span>
        <button
          v-if="view?.captchaAvailable"
          type="button"
          class="primary"
          :disabled="busy"
          @click="captcha"
        >
          {{ busy ? '请在验证窗口中完成操作…' : '打开安全验证' }}
        </button>
        <button v-else type="button" :disabled="busy || !canRefresh" @click="refresh">
          重新生成二维码
        </button>
      </div>
      <div v-else-if="status === 'authorized'" class="security-qr-overlay" role="status">
        <span class="device-spinner" aria-hidden="true" /><span>正在核对账号…</span>
      </div>
      <div v-else class="security-qr-overlay" role="status">
        <span>{{
          status === 'expired'
            ? '二维码已失效'
            : status === 'error' && qr
              ? '服务端异常，请稍后再试'
              : '暂时无法显示验证二维码'
        }}</span>
        <button v-if="canRefresh" type="button" class="primary" :disabled="busy" @click="refresh">
          点击重试
        </button>
        <span v-else>请返回手机登录后重试</span>
      </div>
    </div>
    <p v-if="error" class="verification-error" role="alert">{{ error }}</p>
    <p
      v-if="!canReconcile && !checking && status !== 'waiting_confirmation'"
      class="security-qr-download"
    >
      使用
      <button type="button" :disabled="opening" @click="openDownload">网易云音乐 APP</button>
      扫码验证
    </p>
    <p v-if="documentError" class="verification-error" role="alert">{{ documentError }}</p>
    <button
      v-if="status === 'waiting_confirmation'"
      type="button"
      class="security-qr-other"
      @click="phone"
    >
      选择其他登录模式 <Icon name="forward" />
    </button>
  </section>
</template>
