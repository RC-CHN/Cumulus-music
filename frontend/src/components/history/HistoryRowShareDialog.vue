<script setup lang="ts">
import { onMounted, ref, useId } from 'vue';
import Icon from '../Icon.vue';
import MemberQrImage from '../MemberQrImage.vue';

const props = defineProps<{ modules?: boolean[][] }>();
type Choice = 'wechat_qr' | 'copy';
const emit = defineEmits<{ choose: [choice: Choice]; close: [] }>();
const dialog = ref<HTMLDialogElement>();
const titleId = useId();
const hintId = useId();
const channels = [
  { choice: 'wechat_qr', label: '微信好友', icon: 'chatPair' },
  { choice: 'copy', label: '复制链接', icon: 'source' },
] as const;
let choosing = false;

onMounted(() => dialog.value?.showModal());

function choose(choice: Choice) {
  if (props.modules || choosing) return;
  choosing = true;
  emit('choose', choice);
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog history-row-share-dialog"
    :class="{ 'history-row-share-wechat': modules }"
    :aria-labelledby="titleId"
    :aria-describedby="modules ? hintId : undefined"
    :data-history-share-dialog="modules ? 'wechat_qr' : 'chooser'"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 v-if="!modules" :id="titleId">分享到</h2>
      <button
        type="button"
        class="icon-button"
        :aria-label="modules ? '关闭微信分享二维码' : '关闭分享'"
        autofocus
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <div v-if="modules" class="history-row-share-qr-content">
      <h2 :id="titleId">分享到微信</h2>
      <MemberQrImage data-history-share-qr :modules="modules" :width="180" label="微信分享二维码" />
      <p :id="hintId">打开微信, 扫一扫</p>
    </div>
    <div v-else>
      <button
        v-for="channel in channels"
        :key="channel.choice"
        type="button"
        class="history-row-share-channel"
        :data-history-share-channel="channel.choice"
        @click="choose(channel.choice)"
      >
        <span class="history-row-share-mark" aria-hidden="true">
          <Icon :name="channel.icon" />
        </span>
        {{ channel.label }}
      </button>
    </div>
  </dialog>
</template>

<style scoped>
.playlist-dialog.history-row-share-dialog {
  width: min(442px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  padding: 0;
}

.history-row-share-dialog header {
  margin: 0;
  padding: 16px 18px 12px 26px;
}

.history-row-share-channel {
  display: flex;
  align-items: center;
  gap: 18px;
  width: 100%;
  height: 64px;
  padding: 0 26px;
  font-size: 16px;
  line-height: 1;
  text-align: left;
}

.history-row-share-channel:hover,
.history-row-share-channel:focus-visible {
  background: var(--surface-hover);
}

.history-row-share-channel:focus-visible {
  outline-offset: -3px;
}

.history-row-share-mark {
  display: grid;
  flex: none;
  place-items: center;
  width: 48px;
  height: 48px;
  border-radius: 50%;
  background: var(--surface-alt);
  color: var(--text);
}

[data-history-share-channel='wechat_qr'] .history-row-share-mark {
  background: #07a85a;
  color: #fff;
}

.history-row-share-mark .icon {
  width: 24px;
  height: 24px;
}

.playlist-dialog.history-row-share-wechat {
  width: min(376px, calc(100vw - 32px));
  height: min(520px, calc(100dvh - 32px));
}

.history-row-share-wechat header {
  justify-content: flex-end;
  padding-bottom: 0;
}

.history-row-share-qr-content {
  display: grid;
  justify-items: center;
  padding: 16px 16px 24px;
  text-align: center;
}

.history-row-share-qr-content h2 {
  margin-bottom: 45px;
  font-size: 20px;
  font-weight: 600;
}

.history-row-share-qr-content svg {
  display: block;
  width: min(180px, 100%);
  height: auto;
}

.history-row-share-qr-content p {
  margin-top: 10px;
  color: var(--muted);
  font-size: 14px;
}
</style>
