<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';

const props = defineProps<{ epoch: string; purpose?: 'password' }>();
const emit = defineEmits<{ close: [] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const modules = shallowRef<boolean[][]>();
const loading = ref(false);
const error = ref('');
const owner = props.epoch;
let active = true;
let opener: HTMLElement | undefined;

const size = computed(() => (modules.value?.length ?? 21) + 8);
const path = computed(
  () =>
    modules.value
      ?.flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x + 4} ${y + 4}h1v1h-1z`] : [])))
      .join(' ') ?? '',
);

function close() {
  if (!active) return;
  active = false;
  dialog.value?.close();
  emit('close');
}

async function load() {
  if (!active || loading.value) return;
  loading.value = true;
  error.value = '';
  try {
    const result = await core.accountBindingQr(owner);
    if (!active || owner !== props.epoch) return;
    if (result.epoch !== owner) {
      error.value = '账号状态已变化，请重新打开扫码设置。';
      return;
    }
    const matrix = result.value.modules;
    if (
      !Array.isArray(matrix) ||
      matrix.length < 21 ||
      matrix.length > 41 ||
      (matrix.length - 21) % 4 !== 0 ||
      !matrix.every(
        (row) =>
          Array.isArray(row) &&
          row.length === matrix.length &&
          row.every((cell) => typeof cell === 'boolean'),
      )
    ) {
      error.value = '暂时无法生成二维码，请重试。';
      return;
    }
    modules.value = matrix;
  } catch {
    if (active) error.value = '暂时无法生成二维码，请重试。';
  } finally {
    if (active) loading.value = false;
  }
}

function retry() {
  closeButton.value?.focus();
  void load();
}

watch(() => props.epoch, close, { flush: 'sync' });
onMounted(() => {
  if (!active) return;
  if (document.activeElement instanceof HTMLElement) opener = document.activeElement;
  dialog.value?.showModal();
  void load();
});
onBeforeUnmount(() => {
  active = false;
  dialog.value?.close();
});
onUnmounted(() => {
  if (owner === props.epoch && opener?.isConnected && document.activeElement === document.body) {
    opener.focus({ preventScroll: true });
  }
});
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog account-binding-qr-dialog"
    aria-labelledby="account-binding-qr-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="account-binding-qr-title">
        {{ purpose === 'password' ? '扫码修改密码' : '扫码设置' }}
      </h2>
      <button
        ref="closeButton"
        class="icon-button"
        aria-label="关闭扫码设置"
        autofocus
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="account-binding-qr-body">
      <p v-if="loading" role="status">正在生成二维码…</p>
      <template v-else-if="error">
        <p role="alert">{{ error }}</p>
        <button class="secondary" data-binding-qr-retry @click="retry">重试</button>
      </template>
      <template v-else-if="modules">
        <svg
          data-account-binding-qr
          role="img"
          aria-label="网易云音乐账号绑定设置二维码"
          :viewBox="`0 0 ${size} ${size}`"
          width="180"
          height="180"
          shape-rendering="crispEdges"
        >
          <rect :width="size" :height="size" fill="#fff" />
          <path :d="path" fill="#000" />
        </svg>
        <p>请打开网易云音乐 App 扫码</p>
        <p class="muted">
          {{
            purpose === 'password'
              ? '请在手机上完成密码修改。'
              : '在手机上完成设置后，可返回刷新绑定信息。'
          }}
        </p>
      </template>
    </div>
  </dialog>
</template>

<style scoped>
.account-binding-qr-dialog {
  width: min(376px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  overflow: auto;
}

.account-binding-qr-body {
  display: grid;
  justify-items: center;
  gap: 10px;
  padding: 20px 0;
  text-align: center;
}

.account-binding-qr-body p {
  margin: 0;
}

.account-binding-qr-body svg {
  display: block;
  width: min(180px, 100%);
  height: auto;
}
</style>
