<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import type { LyricBountyAction, LyricBountyOpening } from '../services/player';
const props = defineProps<{
  opening: LyricBountyOpening & { action: LyricBountyAction };
  sending: boolean;
  canConfirm: boolean;
}>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
const close = ref<HTMLButtonElement>();
const title = computed(
  () =>
    ({
      missing_lyrics: '求歌词',
      scrolling: '求滚动',
      translation: '求翻译',
    })[props.opening.action],
);
const request = computed(() => (props.opening.action === 'translation' ? '翻译歌词' : '补歌词'));
onMounted(() => {
  dialog.value?.showModal();
  close.value?.focus();
});
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog lyric-bounty-dialog"
    :data-bounty-action="opening.action"
    :data-bounty-status="sending ? 'sending' : (opening.status ?? 'confirm')"
    :aria-busy="sending"
    aria-labelledby="lyric-bounty-title"
    @cancel.prevent="emit('close')"
    @keydown.esc.stop
  >
    <header>
      <h2 id="lyric-bounty-title">{{ title }}</h2>
    </header>
    <div class="lyric-bounty-copy">
      <template v-if="opening.id || sending">
        <p>悬赏 1 云贝让大家来帮你{{ request }}，是否继续？</p>
        <p class="muted">（若30天内歌词未补充，云贝将退还给您）</p>
        <p v-if="sending" role="status">正在发送…</p>
        <p v-else-if="opening.status === 'not_sent'" role="status">上一次请求未发送。</p>
      </template>
      <p v-else-if="opening.status === 'accepted'" role="status">反馈信息已发送</p>
      <p v-else-if="opening.status === 'not_sent'" role="status">
        请求未发送。确认联网后，请重新打开并确认。
      </p>
      <p v-else role="status">无法确认悬赏是否已发送，请勿重复提交。</p>
    </div>
    <footer>
      <button
        ref="close"
        type="button"
        class="secondary"
        :disabled="sending"
        @click="emit('close')"
      >
        {{ opening.id || sending ? '取消' : '关闭' }}
      </button>
      <button
        v-if="opening.id || sending"
        type="button"
        class="primary"
        :disabled="!canConfirm"
        @click="emit('confirm')"
      >
        确定
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.lyric-bounty-dialog header,
.lyric-bounty-dialog footer {
  justify-content: center;
}
.lyric-bounty-copy {
  padding: 12px 24px;
  text-align: center;
}
</style>
