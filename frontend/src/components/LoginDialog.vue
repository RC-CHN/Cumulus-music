<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { LoginMode } from '../services/auth';
import Icon from './Icon.vue';
import QrLogin from './login/QrLogin.vue';
import CredentialLogin from './login/CredentialLogin.vue';
const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{ close: []; authorized: []; accounts: [] }>();
const dialog = ref<HTMLDialogElement>();
const mode = ref<'qr' | Exclude<LoginMode, 'saved_account'>>('qr');
const title = computed(() =>
  mode.value === 'qr' ? '扫码登录' : mode.value === 'email' ? '邮箱登录' : '手机登录',
);
let focused: HTMLElement | null = null;
watch(
  () => props.epoch,
  () => emit('close'),
);
onMounted(() => {
  focused = document.activeElement as HTMLElement;
  dialog.value?.showModal();
});
onUnmounted(() => focused?.focus());
</script>
<template>
  <dialog
    ref="dialog"
    class="login-dialog"
    aria-labelledby="login-title"
    @cancel.prevent="emit('close')"
  >
    <button class="icon-button dialog-close" aria-label="关闭登录" @click="emit('close')">
      <Icon name="close" />
    </button>
    <div class="brand-mark">C</div>
    <h2 id="login-title">{{ title }}</h2>
    <nav class="login-methods" aria-label="登录方式">
      <button :aria-pressed="mode === 'qr'" @click="mode = 'qr'">扫码</button>
      <button :aria-pressed="mode.startsWith('phone')" @click="mode = 'phone_sms'">手机</button>
      <button :aria-pressed="mode === 'email'" @click="mode = 'email'">邮箱</button>
    </nav>
    <Transition name="tab" mode="out-in">
      <div :key="mode">
        <QrLogin v-if="mode === 'qr'" @authorized="emit('authorized')" />
        <CredentialLogin
          v-else
          :epoch="epoch"
          :mode="mode"
          @mode="mode = $event"
          @authorized="emit('authorized')"
          @qr="mode = 'qr'"
        />
      </div>
    </Transition>
    <button class="login-accounts" @click="emit('accounts')">切换已保存账号</button>
    <p class="login-note">Cumulus · 社区客户端</p>
  </dialog>
</template>
