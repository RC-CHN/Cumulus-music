<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import type { FmReasons } from '../../services/fm';
import Icon from '../Icon.vue';
const props = defineProps<{
  name: string;
  reasons?: FmReasons;
  loading: boolean;
  busy: boolean;
  changed: boolean;
  review: boolean;
  canSubmit: boolean;
  issue: string;
  reasonIssue: string;
}>();
const emit = defineEmits<{
  close: [];
  submit: [key?: string];
  reload: [];
  blacklist: [];
  review: [];
}>();
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
watch([() => props.busy, () => props.loading], async ([busy, loading]) => {
  const element = dialog.value;
  if (!element) return;
  if (busy || loading) {
    if (element.contains(document.activeElement)) element.focus({ preventScroll: true });
  } else {
    await nextTick();
    if (
      element.isConnected &&
      !element.inert &&
      !props.busy &&
      !props.loading &&
      (document.activeElement === element || document.activeElement === document.body)
    )
      cancel.value?.focus({ preventScroll: true });
  }
});
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog fm-dialog"
    aria-labelledby="fm-feedback-title"
    aria-describedby="fm-feedback-description"
    :aria-busy="busy"
    @keydown.esc.stop
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 id="fm-feedback-title">减少推荐</h2>
      <button class="text-button" :disabled="busy" @click="emit('blacklist')">音乐黑名单</button>
      <button class="icon-button" aria-label="关闭减少推荐" :disabled="busy" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <p id="fm-feedback-description">
      减少推荐「{{ name }}」，并播放下一首。之后可以在减少推荐列表中恢复。
    </p>
    <p v-if="changed" class="form-warning" role="status">
      当前歌曲或频道已切换，请关闭后重新打开此窗口。
    </p>
    <p v-else-if="review" class="form-warning" role="status">
      有反馈尚待核对，请先查看反馈状态，避免重复提交。
    </p>
    <p v-if="loading && !changed" class="muted" role="status">正在读取反馈选项…</p>
    <div v-if="reasons && !changed" class="fm-reasons">
      <div class="fm-reason-grid" aria-label="歌手反馈">
        <button
          v-for="reason in reasons.options.filter((r) => r.artistId)"
          :key="reason.key"
          :data-reason-key="reason.key"
          :data-artist-id="reason.artistId"
          class="fm-reason"
          :disabled="!canSubmit"
          :title="reason.label"
          :aria-label="reason.label"
          @click="emit('submit', reason.key)"
        >
          {{ reason.label }}
        </button>
      </div>
      <h3>歌曲反馈</h3>
      <div class="fm-reason-grid" aria-label="歌曲反馈">
        <button
          v-for="reason in reasons.options.filter((r) => !r.artistId)"
          :key="reason.key"
          :data-reason-key="reason.key"
          class="fm-reason"
          :disabled="!canSubmit"
          :title="reason.label"
          :aria-label="reason.label"
          @click="emit('submit', reason.key)"
        >
          {{ reason.label }}
        </button>
      </div>
      <p v-if="!reasons.options.length" class="muted">暂无更多反馈选项，仍可仅减少此歌曲。</p>
    </div>
    <p v-if="reasonIssue && !changed" class="form-warning" role="alert">
      {{ reasonIssue }}
      <button class="text-button" :disabled="busy || loading" @click="emit('reload')">
        重新加载选项
      </button>
    </p>
    <p v-if="issue" class="form-warning" role="alert">{{ issue }}</p>
    <p v-if="review" class="fm-feedback-review">
      <button class="text-button" :disabled="busy" @click="emit('review')">查看反馈状态</button>
    </p>
    <footer>
      <button ref="cancel" class="secondary" :disabled="busy" @click="emit('close')">取消</button>
      <button class="primary" :disabled="!canSubmit" @click="emit('submit')">
        {{ busy ? '正在提交…' : '仅减少此歌曲' }}
      </button>
    </footer>
  </dialog>
</template>
<style scoped>
.fm-dialog {
  width: 410px;
  max-width: calc(100vw - 32px);
}
.fm-dialog:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.fm-dialog header {
  gap: 10px;
}
.fm-dialog h2 {
  flex: 1;
}
.fm-dialog header .text-button {
  font-size: 12px;
}
.fm-dialog p {
  line-height: 1.7;
  overflow-wrap: anywhere;
}
.fm-reasons {
  margin: 20px 0;
}
.fm-reasons h3 {
  margin: 22px 0 10px;
  font-size: 15px;
}
.fm-reason-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.fm-reason {
  height: 44px;
  min-width: 0;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--surface-hover);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  line-height: 1.5;
}
.fm-dialog .form-warning {
  padding: 10px 12px;
  background: var(--accent-soft);
  border: 1px solid var(--line);
  border-radius: 8px;
}
.fm-reason:hover:not(:disabled) {
  color: var(--accent);
  background: var(--accent-soft);
}
.fm-feedback-review {
  margin-top: 12px;
}
@media (max-width: 440px) {
  .fm-reason-grid {
    grid-template-columns: 1fr;
  }
}
</style>
