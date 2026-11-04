<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { useCore } from '../services/core';
import { failure } from '../services/contracts';
import { validMemberQr } from '../services/member';
import Icon from './Icon.vue';
import MemberQrImage from './MemberQrImage.vue';

const props = defineProps<{ offer?: { epoch: string; revision: string } }>();
const emit = defineEmits<{ close: []; refreshOffer: [] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const matrix = shallowRef<boolean[][]>();
const status = ref<'loading' | 'ready' | 'failed' | 'stale'>('loading');
let active = true;
let reading = false;

function close() {
  active = false;
  emit('close');
}

async function load() {
  if (!active || reading) return;
  reading = true;
  status.value = 'loading';
  try {
    const offer = props.offer;
    const result = offer
      ? await core.memberCardOfferQr(offer.epoch, offer.revision)
      : { value: await core.memberPortalQr() };
    if (!active) return;
    if (offer && (!('epoch' in result) || result.epoch !== offer.epoch)) {
      status.value = 'stale';
      return;
    }
    const { modules } = result.value;
    if (!validMemberQr(modules)) {
      status.value = 'failed';
      return;
    }
    matrix.value = modules;
    status.value = 'ready';
  } catch (error) {
    if (active) {
      const code = failure(error).code;
      status.value =
        props.offer && ['stale', 'session_expired', 'profile_changed'].includes(code)
          ? 'stale'
          : 'failed';
    }
  } finally {
    reading = false;
  }
}

function retry() {
  closeButton.value?.focus({ preventScroll: true });
  void load();
}

function refreshOffer() {
  close();
  emit('refreshOffer');
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
    :class="{ 'member-offer-qr-dialog': offer }"
    :aria-labelledby="offer ? undefined : 'member-portal-title'"
    :aria-label="offer ? '会员优惠二维码' : undefined"
    @cancel.prevent="close"
  >
    <header>
      <h2 v-if="!offer" id="member-portal-title">前往会员中心享20+特权</h2>
      <button
        ref="closeButton"
        class="icon-button"
        :aria-label="offer ? '关闭会员优惠二维码' : '关闭会员中心二维码'"
        autofocus
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="member-portal-content">
      <p v-if="status === 'loading'" role="status">正在生成二维码…</p>
      <template v-else-if="status === 'stale'">
        <p role="alert">优惠入口已更新，请重新读取。</p>
        <button class="secondary" @click="refreshOffer">重读优惠</button>
      </template>
      <template v-else-if="status === 'failed'">
        <p role="alert">暂时无法生成会员中心二维码。</p>
        <button class="secondary" @click="retry">重试</button>
      </template>
      <template v-else-if="matrix">
        <MemberQrImage
          :modules="matrix"
          :width="offer ? 180 : 155"
          :label="offer ? '网易云音乐会员优惠二维码' : '网易云音乐会员中心二维码'"
        />
        <p>{{ offer ? '请打开 网易云音乐APP 扫码' : '使用云音乐 App 扫描二维码' }}</p>
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
.member-offer-qr-dialog {
  width: min(376px, calc(100vw - 32px));
  height: min(520px, calc(100dvh - 32px));
  padding: 16px 18px;
}
.member-offer-qr-dialog header {
  justify-content: flex-end;
  margin-bottom: 0;
}
.member-offer-qr-dialog .member-portal-content {
  min-height: 0;
  margin-top: 0;
  padding: clamp(16px, 9vh, 58px) 0 16px;
  background: transparent;
  font-size: 14px;
}
.member-offer-qr-dialog .member-portal-content svg + p {
  margin-top: 10px;
}
.member-portal-content svg {
  display: block;
}
.member-portal-content svg + p {
  margin: 8px 0 0;
}
</style>
