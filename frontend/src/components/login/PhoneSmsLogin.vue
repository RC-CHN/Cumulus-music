<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRefs, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import { useControlFocus } from '../../composables/useControlFocus';
import { useNicknameDraft } from '../../composables/useNicknameDraft';
import type { LoginCommand, PhoneEntryStep } from '../../services/auth';
import type { LoginIdentity } from '../../services/loginIdentity';
import {
  phoneError,
  type LoginCountry,
  type LoginDocument,
} from '../../services/loginPresentation';
import { nicknameError } from '../../services/phoneReuse';
import { nicknameFormatIssue } from '../../services/nickname';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';
import LoginConsent from './LoginConsent.vue';
import LoginCountryPicker from './LoginCountryPicker.vue';
import NicknameEditor from './NicknameEditor.vue';
const props = defineProps<{
  epoch: string;
  model: LoginModel;
  identity: LoginIdentity;
  countries: LoginCountry[];
  countryLoading: boolean;
  countryError: string;
}>();
const emit = defineEmits<{
  country: [value: LoginCountry];
  retryCountries: [];
  password: [];
  stage: [value: PhoneEntryStep];
}>();
const core = useCore();
const { view, busy, error, seconds } = props.model;
const { phone, country, remember } = toRefs(props.identity);
const entry = computed(() => view.value?.phoneEntry);
const root = ref<HTMLElement>();
const consent = ref<{ focus: () => void }>();
const code = ref(''),
  validation = ref('');
const codeEdited = ref(false),
  consentRequired = ref(false),
  openingDocument = ref(false);
const action = ref<
  'start' | 'retry' | 'resend' | 'code' | 'hint' | 'register' | 'reconcile' | null
>(null);
let alive = true,
  generation = 0;
const phase = computed(() => entry.value?.phase);
const nicknameDraft = useNicknameDraft(
  () => {
    const current = entry.value;
    if (
      !view.value ||
      !current ||
      !['nickname', 'hinting', 'checking_nickname', 'registering'].includes(current.phase)
    )
      return null;
    return {
      owner: JSON.stringify([view.value.operationId, current.id]),
      suggestion: current.suggestedNickname,
    };
  },
  () => {
    for (const input of root.value?.querySelectorAll<HTMLInputElement>('[aria-label="注册昵称"]') ??
      [])
      input.value = '';
  },
);
const { name, edited, recommendationError } = nicknameDraft;
const nameUnavailable = computed(() => entry.value?.nicknameError === 'taken' && !edited.value);
const group = computed(() => {
  if (action.value === 'start' || action.value === 'retry' || phase.value === 'checking_account')
    return 'preparing';
  if (phase.value === 'check_failed') return 'check_failed';
  if (phase.value === 'recovery_required') return 'recovery';
  if (
    phase.value &&
    ['nickname', 'hinting', 'checking_nickname', 'registering'].includes(phase.value)
  )
    return 'nickname';
  return entry.value ? 'code' : 'number';
});
const step = computed<PhoneEntryStep>(() =>
  group.value === 'nickname'
    ? 'nickname'
    : group.value === 'recovery'
      ? 'recovery'
      : group.value === 'number'
        ? 'number'
        : 'code',
);
watch(step, (value) => emit('stage', value), { immediate: true });
watch(group, (value, previous) => {
  if (previous === 'code' && value !== 'code') code.value = '';
});
const disabled = computed(() => busy.value || !!action.value || !view.value);
const numberDisabled = computed(
  () => disabled.value || props.countryLoading || !props.countries.length,
);
const issueText = computed(() => {
  if (validation.value) return validation.value;
  if (phase.value === 'nickname' && entry.value?.nicknameError && !edited.value)
    return nicknameError(entry.value.nicknameError);
  const issue = entry.value?.issue;
  if (codeEdited.value && (issue === 'code_invalid' || issue === 'login_rejected')) return '';
  if (entry.value?.nicknameError === 'phone_registered')
    return '手机号已经被注册，请使用短信验证码登录';
  if (!issue || issue === 'cooldown') return error.value;
  return {
    check_failed: '暂时无法检查手机号，请重试',
    send_failed: '验证码发送失败，请重新获取',
    send_unconfirmed: '验证码发送结果未确认，请先查看短信；冷却结束后可重新获取',
    daily_limit: '当天发送验证码的条数超过限制',
    five_per_day: '每个手机号一天只能发5条验证码',
    code_invalid: '验证码错误，请重新输入',
    verification_unconfirmed: '短信校验结果未确认，请检查网络后重试',
    login_rejected:
      entry.value?.code === 503 ? '验证码错误，请重新输入' : '登录未通过，请检查验证码后重试',
    login_unconfirmed: '登录结果未确认，请先核对登录状态',
  }[issue];
});
watch([phone, country, code], () => {
  validation.value = '';
});
watch(
  () => props.identity.consent,
  (value) => {
    if (value) consentRequired.value = false;
  },
);
function clearSecrets() {
  code.value = '';
  nicknameDraft.clear();
  for (const input of root.value?.querySelectorAll<HTMLInputElement>(
    '[autocomplete="one-time-code"],[aria-label="注册昵称"]',
  ) ?? [])
    input.value = '';
}
defineExpose({ clearSecrets });
function clearCode() {
  code.value = '';
  codeEdited.value = true;
  root.value?.querySelector<HTMLElement>('[aria-label="短信验证码"]')?.focus();
}
function leaving(element: Element) {
  (element as HTMLElement).inert = true;
  for (const input of element.querySelectorAll<HTMLInputElement>(
    '[autocomplete="one-time-code"],[aria-label="注册昵称"]',
  ))
    input.value = '';
}
async function focusStep() {
  await nextTick();
  if (!alive || busy.value || document.activeElement !== root.value) return;
  const selector =
    group.value === 'code'
      ? '[aria-label="短信验证码"]'
      : group.value === 'nickname'
        ? '[aria-label="注册昵称"]'
        : group.value === 'number'
          ? '[aria-label="登录手机号"]'
          : '.phone-entry-content button:not(:disabled)';
  const target = Array.from(root.value?.querySelectorAll<HTMLElement>(selector) ?? []).find(
    (node) => !node.closest('[inert]') && !node.matches(':disabled'),
  );
  target?.focus({ preventScroll: true });
}
watch(busy, () => {
  void focusStep();
});
onMounted(() => {
  const focused = document.activeElement;
  const parent = root.value?.closest('.login-flow');
  // Returning from an SDK challenge replaces its focused button while this
  // parent flow survives. Keep ownership through the asynchronous native reset;
  // persistent method/close controls keep their own focus.
  if (
    root.value?.closest('dialog[open]') &&
    (focused === document.body || !focused?.isConnected || parent?.contains(focused))
  ) {
    root.value.focus({ preventScroll: true });
    void focusStep();
  }
});
function agreed() {
  if (props.identity.consent) return true;
  consentRequired.value = true;
  consent.value?.focus();
  return false;
}
async function run(command: LoginCommand, pending: NonNullable<typeof action.value>) {
  if (!alive || disabled.value || root.value?.closest('[inert]')) return;
  root.value?.focus({ preventScroll: true });
  const serial = generation;
  action.value = pending;
  validation.value = '';
  try {
    return await props.model.run(command);
  } finally {
    if (alive && serial === generation) {
      action.value = null;
      await focusStep();
    }
  }
}
async function startNumber() {
  if (numberDisabled.value || !agreed()) return;
  validation.value = phoneError(props.countries, phone.value.trim(), country.value);
  if (validation.value) {
    root.value?.querySelector<HTMLElement>('[aria-label="登录手机号"]')?.focus();
    return;
  }
  await run(
    { kind: 'phone_start', phone: phone.value.trim(), country_code: country.value },
    'start',
  );
}
async function submitCode() {
  const id = entry.value?.id;
  if (!id || disabled.value || !agreed()) return;
  if (!/^[0-9]{4,8}$/.test(code.value)) {
    validation.value = '请输入4至8位短信验证码';
    root.value?.querySelector<HTMLElement>('[aria-label="短信验证码"]')?.focus();
    return;
  }
  const value = code.value;
  code.value = '';
  codeEdited.value = false;
  await run({ kind: 'phone_code', entry_id: id, code: value, remember: remember.value }, 'code');
}
async function resend() {
  const id = entry.value?.id;
  if (id && !seconds.value && agreed()) {
    codeEdited.value = false;
    await run({ kind: 'phone_resend', entry_id: id }, 'resend');
  }
}
async function hint() {
  const id = entry.value?.id;
  if (!id || disabled.value || phase.value !== 'nickname') return;
  const request = nicknameDraft.recommendation();
  await run({ kind: 'phone_nickname_hint', entry_id: id }, 'hint');
  if (alive) nicknameDraft.acceptRecommendation(request, entry.value?.suggestedNickname);
}
async function register(useDefault = false) {
  const id = entry.value?.id;
  if (
    !id ||
    disabled.value ||
    phase.value !== 'nickname' ||
    (useDefault
      ? !entry.value?.canSkipNickname
      : nameUnavailable.value || !!nicknameFormatIssue(name.value)) ||
    !agreed()
  )
    return;
  const nickname = useDefault ? null : name.value;
  const previous = entry.value;
  nicknameDraft.submitted();
  const next = await run({ kind: 'phone_register', entry_id: id, nickname }, 'register');
  // If both the command reply and the recovery snapshot are missing, the old
  // projection cannot justify retaining a registration draft for another try.
  if (alive && !next && error.value && entry.value === previous) nicknameDraft.clear();
}
async function startOver() {
  if (!alive) return;
  const serial = ++generation;
  root.value?.focus({ preventScroll: true });
  clearSecrets();
  edited.value = false;
  validation.value = '';
  action.value = null;
  await props.model.start(props.epoch, 'phone_sms');
  if (alive && serial === generation) await focusStep();
}
const documentFocus = useControlFocus(
  () => (alive ? props.identity : null),
  () => root.value?.querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)'),
);
async function openDocument(kind: LoginDocument) {
  if (!alive || openingDocument.value) return;
  const restore = documentFocus.capture(document.activeElement);
  openingDocument.value = true;
  validation.value = '';
  try {
    await core.openLoginDocument(kind);
  } catch {
    if (alive) validation.value = '浏览器未能打开，请检查默认浏览器后重试';
  } finally {
    if (alive) openingDocument.value = false;
    await nextTick();
    restore();
  }
}
onBeforeUnmount(() => {
  alive = false;
  generation++;
  clearSecrets();
  if (root.value) root.value.inert = true;
  emit('stage', 'number');
});
</script>
<template>
  <section
    ref="root"
    class="phone-sms-login"
    tabindex="-1"
    :data-phone-phase="phase ?? 'number'"
    :data-phone-step="step"
    :data-phone-issue="entry?.issue"
    :aria-busy="busy"
  >
    <button
      v-if="group !== 'number'"
      type="button"
      class="reuse-back"
      aria-label="修改手机号"
      @click="startOver"
    >
      <Icon name="back" />
    </button>
    <Transition name="reuse-step" mode="out-in" @before-leave="leaving" @after-enter="focusStep">
      <div v-if="group === 'number'" key="number" class="phone-entry-content">
        <form class="credential-form" novalidate @submit.prevent="startNumber">
          <div class="login-phone">
            <LoginCountryPicker
              :countries="countries"
              :alpha="identity.countryAlpha"
              :disabled="numberDisabled"
              @select="emit('country', $event)"
            />
            <label
              ><span class="sr-only">手机号</span
              ><input
                v-model="phone"
                aria-label="登录手机号"
                type="tel"
                autocomplete="tel-national"
                placeholder="请输入手机号"
                pattern="[0-9]{1,32}"
                maxlength="32"
                required
                :disabled="numberDisabled"
            /></label>
          </div>
          <p v-if="countryError" class="credential-error" role="alert">
            {{ countryError
            }}<button v-if="!countries.length" type="button" @click="emit('retryCountries')">
              重新加载
            </button>
          </p>
          <div class="login-options">
            <p v-if="issueText" class="credential-error" role="alert">{{ issueText }}</p>
            <label v-else
              ><input v-model="remember" type="checkbox" :disabled="disabled" />自动登录</label
            >
            <div class="login-option-links">
              <button type="button" :disabled="disabled" @click="emit('password')">密码登录</button>
            </div>
          </div>
          <button class="primary login-submit" type="submit" :disabled="numberDisabled">
            验证码登录
          </button>
          <LoginConsent
            ref="consent"
            v-model="identity.consent"
            :invalid="consentRequired"
            :disabled="disabled"
            :opening="openingDocument || busy"
            @open="openDocument"
          />
        </form>
      </div>
      <div v-else-if="group === 'code'" key="code" class="phone-entry-content">
        <form class="credential-form" novalidate @submit.prevent="submitCode">
          <label
            ><span class="sr-only">验证码</span
            ><span class="login-code"
              ><span class="phone-code-field"
                ><input
                  v-model="code"
                  aria-label="短信验证码"
                  placeholder="请输入验证码"
                  inputmode="numeric"
                  autocomplete="one-time-code"
                  pattern="[0-9]{4,8}"
                  maxlength="8"
                  required
                  :disabled="disabled || phase !== 'code_required'"
                  @input="codeEdited = true" /><button
                  v-if="code"
                  class="phone-input-clear"
                  type="button"
                  aria-label="清空验证码"
                  :disabled="disabled"
                  @click="clearCode"
                >
                  <Icon name="close" /></button></span
              ><button
                type="button"
                :disabled="disabled || seconds > 0 || phase !== 'code_required'"
                @click="resend"
              >
                {{ seconds ? `${seconds} 秒后重发` : '获取验证码' }}
              </button></span
            ></label
          >
          <div class="login-options">
            <p v-if="issueText" class="credential-error" role="alert">{{ issueText }}</p>
            <label v-else
              ><input v-model="remember" type="checkbox" :disabled="disabled" />自动登录</label
            >
            <div class="login-option-links">
              <button type="button" :disabled="disabled" @click="emit('password')">密码登录</button>
            </div>
          </div>
          <button
            class="primary login-submit"
            type="submit"
            :disabled="disabled || phase !== 'code_required'"
          >
            {{ busy ? '正在登录…' : '登录' }}
          </button>
          <p class="phone-entry-note">未注册的手机号登录后将自动创建账号</p>
          <p v-if="entry?.registrationBlocked" class="reuse-boundary" role="status">
            上次注册结果未确认，本次仅进行短信登录核对。
          </p>
          <LoginConsent
            ref="consent"
            v-model="identity.consent"
            :invalid="consentRequired"
            :disabled="disabled"
            :opening="openingDocument || busy"
            @open="openDocument"
          />
        </form>
      </div>
      <div
        v-else-if="group === 'nickname'"
        key="nickname"
        class="phone-entry-content reuse-nickname"
      >
        <NicknameEditor
          :model-value="name"
          :disabled="disabled"
          :busy="busy || !!action"
          :error="recommendationError || issueText"
          :allow-default="!!entry?.canSkipNickname"
          :name-unavailable="nameUnavailable"
          @update:model-value="nicknameDraft.edit"
          @random="hint"
          @submit="register"
        >
          <button type="button" class="reuse-cancel" @click="startOver">取消</button>
        </NicknameEditor>
      </div>
      <div
        v-else-if="group === 'preparing'"
        key="preparing"
        class="phone-entry-content reuse-loading"
        role="status"
      >
        <span class="device-spinner" aria-hidden="true" /><span>正在准备验证码登录…</span>
      </div>
      <div
        v-else-if="group === 'check_failed'"
        key="check_failed"
        class="phone-entry-content reuse-failed"
        role="alert"
      >
        <Icon name="info" class="reuse-info" />
        <p>{{ issueText || '暂时无法检查手机号，请重试' }}</p>
        <button
          type="button"
          class="primary reuse-primary"
          :disabled="disabled"
          @click="entry && run({ kind: 'phone_retry', entry_id: entry.id }, 'retry')"
        >
          {{ busy ? '正在检查…' : '重试' }}
        </button>
      </div>
      <div v-else key="recovery" class="phone-entry-content reuse-failed" role="status">
        <Icon name="info" class="reuse-info" />
        <p>
          {{
            entry?.canReconcile
              ? '无法确认上次操作结果，请先核对登录状态。'
              : '无法确认上次操作结果，请重新短信登录。'
          }}
        </p>
        <button
          v-if="entry?.canReconcile"
          type="button"
          class="primary reuse-primary"
          :disabled="disabled"
          @click="run({ kind: 'reconcile' }, 'reconcile')"
        >
          {{ busy ? '正在核对…' : '核对登录状态' }}
        </button>
        <p v-else class="reuse-boundary">重新登录将核对账号，不会重复注册。</p>
        <p v-if="error" class="verification-error" role="alert">{{ error }}</p>
        <button type="button" class="reuse-cancel" @click="startOver">重新短信登录</button>
      </div>
    </Transition>
  </section>
</template>
