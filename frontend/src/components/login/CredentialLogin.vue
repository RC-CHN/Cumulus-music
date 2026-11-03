<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRefs, watch } from 'vue';
import { useCore } from '../../services/core';
import { useLogin } from '../../composables/useLogin';
import { useControlFocus } from '../../composables/useControlFocus';
import { loginMessage, type LoginMode, type PhoneEntryStep } from '../../services/auth';
import type { LoginIdentity } from '../../services/loginIdentity';
import {
  phoneError,
  presentLoginNotice,
  type LoginCountry,
  type LoginDocument,
} from '../../services/loginPresentation';
import LoginChallenge from './LoginChallenge.vue';
import LoginCountryPicker from './LoginCountryPicker.vue';
import LoginEmailInput from './LoginEmailInput.vue';
import LoginConsent from './LoginConsent.vue';
import PhoneSmsLogin from './PhoneSmsLogin.vue';
import { reuseTitle } from '../../services/phoneReuse';
const props = defineProps<{
  epoch: string;
  mode: Exclude<LoginMode, 'saved_account'>;
  identity: LoginIdentity;
  countries: LoginCountry[];
  countryLoading: boolean;
  countryError: string;
}>();
const emit = defineEmits<{
  authorized: [];
  mode: [value: Exclude<LoginMode, 'saved_account'>];
  qr: [];
  safety: [value: string | null];
  country: [value: LoginCountry];
  retryCountries: [];
  forgot: [];
  phoneStage: [value: PhoneEntryStep];
}>();
const core = useCore();
const model = useLogin(core, () => emit('authorized'));
const { view, busy, error } = model;
const notice = computed(() => presentLoginNotice(view.value));
const securityQrActive = computed(
  () => view.value?.status === 'security_qr' || !!view.value?.securityQr,
);
const penaltyActive = computed(() => view.value?.status === 'penalty' || !!view.value?.penalty);
const { phone, country, email, remember } = toRefs(props.identity);
const password = ref('');
const phoneFlow = ref<{ clearSecrets: () => void }>();
const root = ref<HTMLElement>();
const phoneInput = ref<HTMLInputElement>(),
  passwordInput = ref<HTMLInputElement>();
const consent = ref<{ focus: () => void }>();
const consentRequired = ref(false),
  validation = ref(''),
  openingDocument = ref(false);
const challenge = ref<{ clearSecrets: () => void }>();
const deviceKind = ref<'confirm' | 'scan' | 'sms' | 'email' | null>(null);
let active = true;
const documentFocus = useControlFocus(
  () => (active ? props.identity : null),
  () => root.value?.querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)'),
);
function deactivate() {
  if (!active) return;
  active = false;
  password.value = '';
  phoneFlow.value?.clearSecrets();
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
const questionActive = computed(
  () => view.value?.status === 'security_question' || !!view.value?.question,
);
const reuseActive = computed(() => view.value?.status === 'phone_reuse' || !!view.value?.reuse);
const reuseHeading = ref<string | null>(null);
watch(
  [
    busy,
    () => view.value?.status,
    () => notice.value?.kind,
    securityQrActive,
    penaltyActive,
    deviceActive,
    deviceKind,
    smsActive,
    emailActive,
    questionActive,
    reuseActive,
    reuseHeading,
    () => view.value?.reuse?.phase,
  ],
  async () => {
    if (active) {
      let title: string | null = null;
      if (notice.value) title = notice.value.kind === 'risk' ? '登录风险提示' : notice.value.title;
      else if (penaltyActive.value) title = '账号安全确认';
      else if (securityQrActive.value) title = '设备验证';
      else if (reuseActive.value)
        title = reuseHeading.value ?? reuseTitle(view.value?.reuse?.phase);
      else if (questionActive.value) title = '回答与你账号相关的问题';
      else if (deviceKind.value === 'email' || emailActive.value) title = '安全邮箱验证';
      else if (deviceKind.value || deviceActive.value || smsActive.value)
        title =
          (deviceKind.value ?? view.value?.device?.kind) === 'scan' ? '身份验证' : '登录安全确认';
      else if (view.value?.status === 'two_factor') title = '登录安全验证';
      emit('safety', title);
    }
    if (
      notice.value ||
      securityQrActive.value ||
      penaltyActive.value ||
      questionActive.value ||
      reuseActive.value ||
      phoneActive.value
    )
      return;
    const owned = root.value?.contains(document.activeElement);
    await nextTick();
    if (!active || !owned || !root.value) return;
    const focused = document.activeElement as HTMLElement | null;
    if (
      focused &&
      focused !== root.value &&
      root.value.contains(focused) &&
      !focused.matches(':disabled')
    )
      return;
    const preferred =
      view.value?.status === 'rejected' && view.value.code === 502
        ? '[aria-label="登录密码"]:not(:disabled)'
        : null;
    const target =
      (preferred ? root.value.querySelector<HTMLElement>(preferred) : null) ??
      root.value.querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)');
    (target ?? root.value).focus({ preventScroll: true });
  },
);
const initial = computed(
  () => !!view.value && ['ready', 'code_sent', 'rejected'].includes(view.value.status),
);
const phoneActive = computed(
  () =>
    !notice.value &&
    !securityQrActive.value &&
    !penaltyActive.value &&
    props.mode === 'phone_sms' &&
    (initial.value || !view.value || !!view.value.phoneEntry),
);
const disabled = computed(
  () =>
    busy.value ||
    !view.value ||
    !initial.value ||
    (props.mode !== 'email' && (props.countryLoading || !props.countries.length)),
);
const formError = computed(
  () =>
    validation.value ||
    (initial.value && view.value?.status === 'rejected'
      ? error.value || loginMessage(view.value)
      : ''),
);
watch(
  () => props.identity.consent,
  (value) => {
    if (value) consentRequired.value = false;
  },
);
watch([phone, country, email, password], () => {
  validation.value = '';
});
function agreed() {
  if (props.identity.consent) return true;
  consentRequired.value = true;
  consent.value?.focus();
  return false;
}
function validPhone() {
  validation.value = phoneError(props.countries, phone.value.trim(), country.value);
  if (validation.value) phoneInput.value?.focus({ preventScroll: true });
  return !validation.value;
}
async function openDocument(kind: LoginDocument) {
  if (!active || openingDocument.value) return;
  const restoreFocus = documentFocus.capture(document.activeElement);
  openingDocument.value = true;
  validation.value = '';
  try {
    await core.openLoginDocument(kind);
  } catch {
    if (active) validation.value = '浏览器未能打开，请检查默认浏览器后重试';
  } finally {
    if (active) openingDocument.value = false;
    await nextTick();
    restoreFocus();
  }
}
async function start() {
  password.value = '';
  phoneFlow.value?.clearSecrets();
  challenge.value?.clearSecrets();
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
  if (!active || disabled.value || props.mode === 'phone_sms' || !agreed()) return;
  if (props.mode !== 'email' && !validPhone()) return;
  if (
    props.mode === 'email' &&
    !root.value?.querySelector<HTMLInputElement>('[aria-label="登录邮箱"]')?.validity.valid
  ) {
    validation.value = '请输入正确的邮箱';
    root.value?.querySelector<HTMLInputElement>('[aria-label="登录邮箱"]')?.focus();
    return;
  }
  if (!password.value) {
    validation.value = '请输入密码';
    passwordInput.value?.focus();
    return;
  }
  const secret = password.value;
  password.value = '';
  phoneFlow.value?.clearSecrets();
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
}
</script>
<template>
  <div ref="root" class="login-flow" tabindex="-1" :aria-busy="busy">
    <PhoneSmsLogin
      v-if="phoneActive"
      ref="phoneFlow"
      :epoch="epoch"
      :model="model"
      :identity="identity"
      :countries="countries"
      :country-loading="countryLoading"
      :country-error="countryError"
      @country="emit('country', $event)"
      @retry-countries="emit('retryCountries')"
      @password="emit('mode', 'phone_password')"
      @stage="emit('phoneStage', $event)"
    />
    <form v-else-if="initial || !view" class="credential-form" novalidate @submit.prevent="submit">
      <LoginEmailInput
        v-if="mode === 'email'"
        v-model="email"
        :disabled="disabled"
        @chosen="passwordInput?.focus()"
      />
      <div v-else class="login-phone">
        <LoginCountryPicker
          :countries="countries"
          :alpha="identity.countryAlpha"
          :disabled="disabled || countryLoading || !countries.length"
          @select="emit('country', $event)"
        />
        <label
          ><span class="sr-only">手机号</span
          ><input
            ref="phoneInput"
            v-model="phone"
            aria-label="登录手机号"
            type="tel"
            autocomplete="tel-national"
            placeholder="请输入手机号"
            pattern="[0-9]{1,32}"
            maxlength="32"
            required
            :disabled="disabled"
        /></label>
      </div>
      <p v-if="mode !== 'email' && countryError" class="credential-error" role="alert">
        {{ countryError }}
        <button v-if="!countries.length" type="button" @click="emit('retryCountries')">
          重新加载
        </button>
      </p>
      <label
        ><span class="sr-only">密码</span
        ><input
          ref="passwordInput"
          v-model="password"
          aria-label="登录密码"
          type="password"
          placeholder="请输入密码"
          autocomplete="current-password"
          maxlength="1024"
          required
          :disabled="disabled"
      /></label>
      <div class="login-options">
        <p v-if="formError" class="credential-error" role="alert">{{ formError }}</p>
        <label v-else><input v-model="remember" type="checkbox" :disabled="busy" />自动登录</label>
        <div class="login-option-links">
          <button
            type="button"
            :disabled="busy || openingDocument"
            @click="mode === 'email' ? openDocument('email_recovery') : emit('forgot')"
          >
            忘记密码
          </button>
          <span v-if="mode === 'phone_password'" aria-hidden="true">|</span>
          <button
            v-if="mode !== 'email'"
            type="button"
            :disabled="busy"
            @click="emit('mode', 'phone_sms')"
          >
            验证码登录
          </button>
        </div>
      </div>
      <button
        class="primary login-submit"
        type="submit"
        :disabled="disabled || (mode !== 'email' && !countries.length)"
      >
        {{ busy ? '正在登录…' : '登录' }}
      </button>
      <LoginConsent
        ref="consent"
        v-model="identity.consent"
        :invalid="consentRequired"
        :disabled="busy"
        :opening="openingDocument || busy"
        @open="openDocument"
      />
    </form>
    <LoginChallenge
      v-else
      ref="challenge"
      :model="model"
      @qr="emit('qr')"
      @back="start"
      @phone="props.mode === 'phone_sms' ? start() : emit('mode', 'phone_sms')"
      @reuse-title="reuseHeading = $event"
      @device="deviceKind = $event"
    />
    <p
      v-if="!notice && !securityQrActive && !penaltyActive"
      class="login-status"
      role="status"
      :data-login-status="view?.status"
    >
      {{
        phoneActive ||
        deviceActive ||
        smsActive ||
        emailActive ||
        questionActive ||
        reuseActive ||
        (initial && view?.status === 'rejected') ||
        view?.status === 'two_factor'
          ? ''
          : error || loginMessage(view)
      }}
    </p>
    <button
      v-if="
        !busy &&
        !notice &&
        !securityQrActive &&
        !penaltyActive &&
        !phoneActive &&
        !deviceActive &&
        !smsActive &&
        !emailActive &&
        !questionActive &&
        !reuseActive &&
        (!view || ['error', 'rejected', 'rate_limited', 'account_disabled'].includes(view.status))
      "
      class="login-restart"
      @click="start"
    >
      重新开始登录
    </button>
  </div>
</template>
