<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

const emit = defineEmits<{ close: [] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const matrix = shallowRef<boolean[][]>();
const status = ref<'loading' | 'ready' | 'failed'>('loading');
let active = true;
let reading = false;

const size = computed(() => (matrix.value?.length ?? 0) + 8);
const path = computed(() =>
  (matrix.value ?? [])
    .flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x + 4} ${y + 4}h1v1h-1z`] : [])))
    .join(' '),
);

function close() {
  active = false;
  emit('close');
}

async function load() {
  if (!active || reading) return;
  reading = true;
  status.value = 'loading';
  try {
    const { modules } = await core.memberPortalQr();
    if (!active) return;
    if (
      !Array.isArray(modules) ||
      modules.length < 21 ||
      modules.length > 41 ||
      (modules.length - 21) % 4 !== 0 ||
      !modules.every(
        (row) =>
          Array.isArray(row) &&
          row.length === modules.length &&
          row.every((cell) => typeof cell === 'boolean'),
      )
    ) {
      status.value = 'failed';
      return;
    }
    matrix.value = modules;
    status.value = 'ready';
  } catch {
    if (active) status.value = 'failed';
  } finally {
    reading = false;
  }
}

function retry() {
  closeButton.value?.focus({ preventScroll: true });
  void load();
}

onMounted(() => {
  dialog.value?.showModal();
  void load();
});
onBeforeUnmount(() => {
  active = false;
});
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog member-portal-dialog"
    aria-labelledby="member-portal-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="member-portal-title">前往会员中心享20+特权</h2>
      <button
        ref="closeButton"
        class="icon-button"
        aria-label="关闭会员中心二维码"
        autofocus
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="member-portal-content">
      <p v-if="status === 'loading'" role="status">正在生成二维码…</p>
      <template v-else-if="status === 'failed'">
        <p role="alert">暂时无法生成会员中心二维码。</p>
        <button class="secondary" @click="retry">重试</button>
      </template>
      <template v-else>
        <svg
          role="img"
          aria-label="网易云音乐会员中心二维码"
          :viewBox="`0 0 ${size} ${size}`"
          width="155"
          height="155"
          shape-rendering="crispEdges"
        >
          <rect :width="size" :height="size" fill="#fff" />
          <path :d="path" fill="#000" />
        </svg>
        <p>使用云音乐 App 扫描二维码</p>
      </template>
    </div>
  </dialog>
</template>

<style scoped>
.member-portal-dialog {
  width: min(442px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  border-radius: 16px;
}
.member-portal-content {
  display: grid;
  justify-items: center;
  align-content: center;
  min-height: 205px;
  margin-top: 12px;
  padding: 20px 12px 12px;
  border-radius: 12px;
  background: var(--surface-alt);
  text-align: center;
  font-size: 13px;
}
.member-portal-content svg {
  display: block;
}
.member-portal-content svg + p {
  margin: 8px 0 0;
}
</style>
