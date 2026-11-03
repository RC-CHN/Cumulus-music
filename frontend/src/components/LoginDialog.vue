<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import LoginStepTransition from './login/LoginStepTransition.vue';
import type { LoginMode, PhoneEntryStep } from '../services/auth';
import type { LoginIdentity } from '../services/loginIdentity';
import { useCore } from '../services/core';
import {
  loadLoginPresentation,
  saveLoginCountry,
  type LoginCountry,
} from '../services/loginPresentation';
import Icon from './Icon.vue';
import QrLogin from './login/QrLogin.vue';
import CredentialLogin from './login/CredentialLogin.vue';
const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{ close: []; authorized: []; accounts: [] }>();
const dialog = ref<HTMLDialogElement>();
const core = useCore();
const mode = ref<'qr' | 'phone_help' | Exclude<LoginMode, 'saved_account'>>('qr');
const safety = ref<string | null>(null);
const phoneStage = ref<PhoneEntryStep>('number');
const identity = reactive<LoginIdentity>({
  phone: '',
  country: '86',
  countryAlpha: 'CN',
  email: '',
  remember: true,
  consent: false,
});
const countries = ref<LoginCountry[]>([]);
const countryLoading = ref(false);
const countryError = ref('');
let countryRevision = 0;
async function loadCountries() {
  if (closing || countryLoading.value) return;
  countryLoading.value = true;
  countryError.value = '';
  try {
    const value = await loadLoginPresentation(core);
    if (closing) return;
    countries.value = value.countries;
    const selected = value.countries.find((row) => row.alpha === value.selectedCountry);
    if (selected) {
      identity.countryAlpha = selected.alpha;
      identity.country = selected.calling;
    }
  } catch {
    if (!closing) countryError.value = '地区加载失败，请重试';
  } finally {
    if (!closing) countryLoading.value = false;
  }
}
function selectCountry(country: LoginCountry) {
  identity.countryAlpha = country.alpha;
  identity.country = country.calling;
  const revision = ++countryRevision;
  countryError.value = '';
  // A rapid series of selections must persist in user-action order, even if
  // the modal changes method or closes while native IPC is completing.
  void saveLoginCountry(core, country.alpha).catch(() => {
    if (!closing && revision === countryRevision)
      countryError.value = '地区选择未保存，当前仍可使用';
  });
}
const flow = ref<{ deactivate: () => void }>();
const panel = ref<HTMLElement>();
let closing = false;
function deactivate() {
  if (panel.value) panel.value.inert = true;
  flow.value?.deactivate();
}
function select(value: typeof mode.value) {
  if (closing || mode.value === value) return;
  deactivate();
  safety.value = null;
  phoneStage.value = 'number';
  mode.value = value;
}
function phone() {
  if (!mode.value.startsWith('phone')) select('phone_sms');
}
function close(destination: 'close' | 'accounts' = 'close') {
  if (closing) return;
  closing = true;
  deactivate();
  if (destination === 'accounts') emit('accounts');
  else emit('close');
}
function entered(element: Element) {
  // Replace lost focus or the panel's temporary focus, but preserve a
  // control the user already reached while the new panel was entering.
  if (closing || panel.value !== element) return;
  const active = document.activeElement;
  if (active === document.body || active === element) {
    const target =
      mode.value === 'phone_help'
        ? dialog.value?.querySelector<HTMLElement>('.login-help-back')
        : element.querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)');
    (target ?? (element as HTMLElement)).focus({ preventScroll: true });
  }
}
const title = computed(() =>
  safety.value
    ? safety.value
    : mode.value === 'qr'
      ? '扫码登录'
      : mode.value === 'email'
        ? '邮箱登录'
        : mode.value === 'phone_sms' && phoneStage.value === 'nickname'
          ? '希望大家怎么称呼你呢？'
          : mode.value === 'phone_sms' && phoneStage.value === 'recovery'
            ? '核对登录状态'
            : mode.value === 'phone_sms' && phoneStage.value === 'code'
              ? '验证码登录'
              : mode.value === 'phone_help'
                ? '忘记密码'
                : '手机登录',
);
let focused: HTMLElement | null = null;
watch(
  () => props.epoch,
  () => close(),
);
onMounted(() => {
  focused = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  void loadCountries();
});
onUnmounted(() => focused?.focus());
onBeforeUnmount(deactivate);
</script>
<template>
  <dialog ref="dialog" class="login-dialog" aria-labelledby="login-title" @cancel.prevent="close()">
    <button class="icon-button dialog-close" aria-label="关闭登录" @click="close()">
      <Icon name="close" />
    </button>
    <button
      v-if="mode === 'phone_help'"
      class="login-help-back"
      aria-label="其他登录方式"
      @click="select('phone_password')"
    >
      ‹ 其他登录方式
    </button>
    <div
      v-if="!safety && phoneStage !== 'nickname' && phoneStage !== 'recovery'"
      class="brand-mark"
    >
      C
    </div>
    <h2 id="login-title">{{ title }}</h2>
    <nav
      v-if="
        !safety && mode !== 'phone_help' && phoneStage !== 'nickname' && phoneStage !== 'recovery'
      "
      class="login-methods"
      aria-label="登录方式"
    >
      <button :aria-pressed="mode === 'qr'" @click="select('qr')">扫码</button>
      <button :aria-pressed="mode.startsWith('phone')" @click="phone">手机</button>
      <button :aria-pressed="mode === 'email'" @click="select('email')">邮箱</button>
    </nav>
    <LoginStepTransition name="tab" @after-enter="entered">
      <div :key="mode" ref="panel" class="login-panel" tabindex="-1">
        <QrLogin v-if="mode === 'qr'" ref="flow" @authorized="emit('authorized')" />
        <div v-else-if="mode === 'phone_help'" class="phone-password-help" tabindex="-1">
          <p>请前往手机端最新版【网易云音乐 App】进行操作</p>
        </div>
        <CredentialLogin
          v-else
          ref="flow"
          :epoch="epoch"
          :mode="mode"
          :identity="identity"
          :countries="countries"
          :country-loading="countryLoading"
          :country-error="countryError"
          @country="selectCountry"
          @retry-countries="loadCountries"
          @forgot="select('phone_help')"
          @mode="select"
          @authorized="emit('authorized')"
          @qr="select('qr')"
          @safety="safety = $event"
          @phone-stage="phoneStage = $event"
        />
      </div>
    </LoginStepTransition>
    <button
      v-if="
        !safety && mode !== 'phone_help' && phoneStage !== 'nickname' && phoneStage !== 'recovery'
      "
      class="login-accounts"
      @click="close('accounts')"
    >
      切换已保存账号
    </button>
    <p v-if="!safety" class="login-note">Cumulus · 社区客户端</p>
  </dialog>
</template>
