<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import PinCode from './PinCode.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ back: [] }>();
const email = computed(() => props.model.view.value?.email);
const root = ref<HTMLElement>();
const pin = ref<{ clear: () => void; focus: () => void }>();
const edited = ref(false),
  submitting = ref(false),
  resending = ref(false);
let alive = true;
const acceptsCode = computed(
  () => !!email.value && ['code_required', 'send_unconfirmed'].includes(email.value.phase),
);
const addressLabel = computed(() => {
  const value = email.value?.mailAddress ?? '';
  if ([...value].length <= 22) return value;
  const split = value.lastIndexOf('@'),
    local = [...value.slice(0, split)],
    host = [...value.slice(split + 1)];
  const domain =
    host.length > 18 ? host.slice(0, 7).join('') + '…' + host.slice(-9).join('') : host.join('');
  return local.slice(0, Math.max(1, 22 - [...domain].length - 2)).join('') + '…@' + domain;
});
function clearSecrets() {
  pin.value?.clear();
}
defineExpose({ clearSecrets });
async function focusCode(id: string) {
  await nextTick();
  if (
    alive &&
    email.value?.id === id &&
    acceptsCode.value &&
    !props.model.busy.value &&
    document.activeElement === root.value
  )
    pin.value?.focus();
}
async function verify(code: string) {
  const value = email.value;
  if (
    !alive ||
    !value ||
    !acceptsCode.value ||
    props.model.busy.value ||
    submitting.value ||
    root.value?.closest('[inert]')
  )
    return;
  clearSecrets();
  edited.value = false;
  submitting.value = true;
  root.value?.focus({ preventScroll: true });
  try {
    await props.model.run({ kind: 'security_mail_verify', verification_id: value.id, code });
  } finally {
    submitting.value = false;
    await focusCode(value.id);
  }
}
async function resend() {
  const value = email.value;
  if (
    !alive ||
    !value ||
    props.model.busy.value ||
    !['verify_limit', 'failed'].includes(value.phase)
  )
    return;
  clearSecrets();
  edited.value = false;
  root.value?.focus({ preventScroll: true });
  resending.value = true;
  try {
    await props.model.run({ kind: 'security_mail_resend', verification_id: value.id });
  } finally {
    resending.value = false;
  }
  if (email.value) await focusCode(email.value.id);
}
watch(
  () => [email.value?.id, email.value?.phase],
  () => {
    if (!acceptsCode.value) clearSecrets();
    if (email.value?.id) void focusCode(email.value.id);
  },
);
onBeforeUnmount(() => {
  alive = false;
  clearSecrets();
});
</script>
<template>
  <section
    ref="root"
    class="email-verification"
    tabindex="-1"
    aria-label="安全邮箱验证"
    :data-email-phase="
      resending
        ? 'sending'
        : submitting
          ? 'verifying'
          : (email?.phase ?? (model.busy.value ? 'sending' : 'failed'))
    "
    :aria-busy="model.busy.value"
  >
    <div
      v-if="!email || email.phase === 'sending' || resending"
      class="device-loading"
      role="status"
    >
      <span v-if="model.busy.value" class="device-spinner" aria-hidden="true" /><span
        class="email-loading-text"
        >{{
          !email && !model.busy.value
            ? '无法开始安全邮箱验证，请返回选择其他验证方式。'
            : '正在发送安全邮箱验证码…'
        }}</span
      >
    </div>
    <template v-else-if="acceptsCode || email.phase === 'verifying'">
      <p
        v-if="email.phase === 'send_unconfirmed'"
        class="email-description email-unconfirmed"
        role="status"
      >
        发送结果尚未确认。若已收到验证码，可以在下方继续验证；本机不会自动重新发送。
      </p>
      <p v-else class="email-description">
        已发送安全验证码至邮箱
        <strong v-if="email.mailAddress" :title="email.mailAddress">{{ addressLabel }}</strong
        ><span v-else>绑定的安全邮箱</span><br /><span class="email-validity"
          >验证码10分钟内有效，</span
        ><span class="email-validity">请尽快打开邮箱获取验证码</span>
      </p>
      <div class="email-pin">
        <p>请输入登录验证码</p>
        <PinCode
          ref="pin"
          label="邮箱验证码"
          group-label="安全邮箱验证码"
          :disabled="model.busy.value || !acceptsCode"
          @complete="verify"
          @edit="edited = true"
        />
        <p v-if="email.answerRejected && !edited" class="verification-error" role="alert">
          {{
            email.code === 4012
              ? '验证码错误，请重新输入'
              : email.code === 509
                ? '验证过于频繁，请稍后再试'
                : '验证码验证未通过，请重试或选择其他验证方式'
          }}
        </p>
        <p v-if="email.phase === 'verifying' || submitting" class="muted" role="status">
          正在校验验证码…
        </p>
      </div>
    </template>
    <template v-else-if="email.phase === 'send_limit'"
      ><p class="email-description" role="alert">邮箱验证码使用达上限，请切换其他验证方式尝试</p>
      <button class="primary device-resend" @click="emit('back')">我知道了</button></template
    >
    <template v-else-if="email.phase === 'verify_limit'"
      ><p class="email-description" role="alert">验证码已过期或验证码输入错误次数过多请重新发送</p>
      <button class="primary device-resend" :disabled="model.busy.value" @click="resend">
        重新发送
      </button></template
    >
    <template v-else-if="email.phase === 'failed'"
      ><p class="email-description" role="alert">
        暂时无法发送邮箱验证码，可以重试或选择其他验证方式。
      </p>
      <button class="primary device-resend" :disabled="model.busy.value" @click="resend">
        重试发送
      </button></template
    >
    <template v-else-if="email.phase === 'recovery_required'"
      ><p class="email-description" role="status">无法确认上次验证码验证结果，请先核对登录状态。</p>
      <button
        class="primary device-resend"
        :disabled="model.busy.value"
        @click="model.run({ kind: 'reconcile' })"
      >
        核对登录状态
      </button></template
    >
    <p
      v-if="
        email &&
        model.error.value &&
        (email.phase === 'recovery_required' || (email.phase === 'code_required' && !edited))
      "
      class="verification-error"
      role="alert"
    >
      {{ model.error.value }}
    </p>
  </section>
</template>
