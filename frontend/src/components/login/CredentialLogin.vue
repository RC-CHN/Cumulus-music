<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRefs, watch } from 'vue';
import { useCore } from '../../services/core';
import { useLogin } from '../../composables/useLogin';
import { loginMessage, type LoginMode } from '../../services/auth';
import type { LoginIdentity } from '../../services/loginIdentity';
import LoginChallenge from './LoginChallenge.vue';
const props = defineProps<{
  epoch: string;
  mode: Exclude<LoginMode, 'saved_account'>;
  identity: LoginIdentity;
}>();
const emit = defineEmits<{
  authorized: [];
  mode: [value: Exclude<LoginMode, 'saved_account'>];
  qr: [];
  safety: [value: string | null];
}>();
const model = useLogin(useCore(), () => emit('authorized'));
const { view, busy, error, seconds } = model;
const { phone, country, email, remember } = toRefs(props.identity);
const password = ref(''),
  code = ref('');
const root = ref<HTMLElement>();
const challenge = ref<{ clearSecrets: () => void }>();
const deviceKind = ref<'confirm' | 'scan' | 'sms' | 'email' | null>(null);
let active = true;
function deactivate() {
  if (!active) return;
  active = false;
  password.value = '';
  code.value = '';
  challenge.value?.clearSecrets();
  if (root.value) {
    root.value.inert = true;
    // The transition can retain detached component DOM for its exit animation.
    // Vue no longer patches those nodes after unmount, so clear their values now.
    for (const input of root.value.querySelectorAll<HTMLInputElement>(
      'input[type="password"],input[autocomplete="one-time-code"]',
    ))
      input.value = '';
    for (const input of root.value.querySelectorAll<HTMLInputElement>('input[name="login-answer"]'))
      input.checked = false;
  }
  model.dispose();
}
defineExpose({ deactivate });
const deviceActive = computed(
  () =>
    !!view.value?.device &&
    !['stopped', 'cancelled', 'stop_unconfirmed'].includes(view.value.device.phase),
);
const smsActive = computed(() => !!view.value?.sms);
const emailActive = computed(() => !!view.value?.email);
watch(
  [busy, () => view.value?.status, deviceActive, deviceKind, smsActive, emailActive],
  async () => {
    if (active) {
      const safety =
        deviceKind.value ||
        deviceActive.value ||
        smsActive.value ||
        emailActive.value ||
        (!!view.value &&
          ['two_factor', 'security_question', 'phone_reuse'].includes(view.value.status));
      emit(
        'safety',
        safety
          ? deviceKind.value === 'email' || emailActive.value
            ? '安全邮箱验证'
            : deviceKind.value || deviceActive.value || smsActive.value
              ? (deviceKind.value ?? view.value?.device?.kind) === 'scan'
                ? '身份验证'
                : '登录安全确认'
              : '登录安全验证'
          : null,
      );
    }
    const owned = root.value?.contains(document.activeElement);
    await nextTick();
    if (!active || !owned || !root.value) return;
    const focused = document.activeElement as HTMLElement | null;
    if (focused && root.value.contains(focused) && !focused.matches(':disabled')) return;
    const target = root.value.querySelector<HTMLElement>(
      'input:not(:disabled),button:not(:disabled)',
    );
    (target ?? root.value).focus({ preventScroll: true });
  },
);
const initial = computed(
  () => !!view.value && ['ready', 'code_sent', 'rejected'].includes(view.value.status),
);
const disabled = computed(() => busy.value || !view.value || !initial.value);
async function start() {
  password.value = '';
  code.value = '';
  await model.start(props.epoch, props.mode);
}
watch(
  () => props.mode,
  () => {
    void start();
  },
);
onMounted(() => {
  void start();
});
onBeforeUnmount(deactivate);
async function submit() {
  if (!active || disabled.value) return;
  const secret = password.value,
    otp = code.value;
  password.value = '';
  code.value = '';
  if (view.value?.status === 'rejected') await model.start(props.epoch, props.mode);
  if (!active) return;
  if (props.mode === 'email')
    await model.run({
      kind: 'email',
      address: email.value.trim(),
      password: secret,
      remember: remember.value,
    });
  else if (props.mode === 'phone_password')
    await model.run({
      kind: 'phone_password',
      phone: phone.value.trim(),
      country_code: country.value,
      password: secret,
      remember: remember.value,
    });
  else
    await model.run({
      kind: 'phone_sms',
      phone: phone.value.trim(),
      country_code: country.value,
      code: otp,
      remember: remember.value,
    });
}
</script>
<template>
  <div ref="root" class="login-flow" tabindex="-1" :aria-busy="busy">
    <form v-if="initial || !view" class="credential-form" @submit.prevent="submit">
      <label v-if="mode === 'email'"
        >邮箱<input
          v-model="email"
          aria-label="登录邮箱"
          type="email"
          autocomplete="username"
          maxlength="254"
          required
          :disabled="disabled"
      /></label>
      <div v-else class="login-phone">
        <label class="country"
          >区号<input
            v-model="country"
            aria-label="国际区号"
            inputmode="numeric"
            pattern="[0-9]{1,4}"
            maxlength="4"
            required
            :disabled="disabled"
        /></label>
        <label
          >手机号<input
            v-model="phone"
            aria-label="登录手机号"
            type="tel"
            autocomplete="tel-national"
            pattern="[0-9]{5,15}"
            maxlength="15"
            required
            :disabled="disabled"
        /></label>
      </div>
      <label v-if="mode !== 'phone_sms'"
        >密码<input
          v-model="password"
          aria-label="登录密码"
          type="password"
          autocomplete="current-password"
          maxlength="1024"
          required
          :disabled="disabled"
      /></label>
      <label v-else
        >验证码<span class="login-code"
          ><input
            v-model="code"
            aria-label="短信验证码"
            inputmode="numeric"
            autocomplete="one-time-code"
            pattern="[0-9]{4,8}"
            maxlength="8"
            required
            :disabled="disabled"
          /><button
            type="button"
            :disabled="
              disabled ||
              seconds > 0 ||
              !/^[0-9]{5,15}$/.test(phone) ||
              !/^[0-9]{1,4}$/.test(country)
            "
            @click="model.run({ kind: 'send_sms', phone, country_code: country })"
          >
            {{ seconds ? `${seconds} 秒后重发` : '获取验证码' }}
          </button></span
        ></label
      >
      <div class="login-options">
        <label><input v-model="remember" type="checkbox" :disabled="busy" />自动登录</label
        ><button
          v-if="mode !== 'email'"
          type="button"
          :disabled="busy"
          @click="emit('mode', mode === 'phone_sms' ? 'phone_password' : 'phone_sms')"
        >
          {{ mode === 'phone_sms' ? '密码登录' : '验证码登录' }}
        </button>
      </div>
      <button class="primary login-submit" type="submit" :disabled="disabled">
        {{ busy ? '正在登录…' : '登录' }}
      </button>
    </form>
    <LoginChallenge
      v-else
      ref="challenge"
      :model="model"
      @qr="emit('qr')"
      @device="deviceKind = $event"
    />
    <p class="login-status" role="status" :data-login-status="view?.status">
      {{
        deviceActive || smsActive || emailActive || view?.status === 'two_factor'
          ? ''
          : error || loginMessage(view)
      }}
    </p>
    <button
      v-if="
        !busy &&
        !deviceActive &&
        !smsActive &&
        !emailActive &&
        (!view || ['error', 'rejected', 'rate_limited', 'account_disabled'].includes(view.status))
      "
      class="login-restart"
      @click="start"
    >
      重新开始登录
    </button>
  </div>
</template>
