<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import type { LoginMode } from '../services/auth';
import type { LoginIdentity } from '../services/loginIdentity';
import Icon from './Icon.vue';
import QrLogin from './login/QrLogin.vue';
import CredentialLogin from './login/CredentialLogin.vue';
const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{ close: []; authorized: []; accounts: [] }>();
const dialog = ref<HTMLDialogElement>();
const mode = ref<'qr' | Exclude<LoginMode, 'saved_account'>>('qr');
const safety = ref<string | null>(null);
const identity = reactive<LoginIdentity>({ phone: '', country: '86', email: '', remember: true });
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
  // An internal switch link leaves with its panel. Keep focus inside the modal;
  // clicks on the persistent method buttons keep their own focus.
  if (document.activeElement === document.body || panel.value?.contains(document.activeElement)) {
    const target = element.querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)');
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
});
onUnmounted(() => focused?.focus());
onBeforeUnmount(deactivate);
</script>
<template>
  <dialog ref="dialog" class="login-dialog" aria-labelledby="login-title" @cancel.prevent="close()">
    <button class="icon-button dialog-close" aria-label="关闭登录" @click="close()">
      <Icon name="close" />
    </button>
    <div v-if="!safety" class="brand-mark">C</div>
    <h2 id="login-title">{{ title }}</h2>
    <nav v-if="!safety" class="login-methods" aria-label="登录方式">
      <button :aria-pressed="mode === 'qr'" @click="select('qr')">扫码</button>
      <button :aria-pressed="mode.startsWith('phone')" @click="phone">手机</button>
      <button :aria-pressed="mode === 'email'" @click="select('email')">邮箱</button>
    </nav>
    <Transition name="tab" mode="out-in" @after-enter="entered">
      <div :key="mode" ref="panel" class="login-panel" tabindex="-1">
        <QrLogin v-if="mode === 'qr'" ref="flow" @authorized="emit('authorized')" />
        <CredentialLogin
          v-else
          ref="flow"
          :epoch="epoch"
          :mode="mode"
          :identity="identity"
          @mode="select"
          @authorized="emit('authorized')"
          @qr="select('qr')"
          @safety="safety = $event"
        />
      </div>
    </Transition>
    <button class="login-accounts" @click="close('accounts')">切换已保存账号</button>
    <p class="login-note">Cumulus · 社区客户端</p>
  </dialog>
</template>
