<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import type { useCommunityDelete } from '../../composables/useCommunityDelete';

const props = defineProps<{ model: ReturnType<typeof useCommunityDelete> }>();
const { status, pending, error } = props.model;
const dialog = ref<HTMLDialogElement>();
const cancel = ref<HTMLButtonElement>();
function focusCancelIfNeeded() {
  const surface = dialog.value;
  const focused = document.activeElement;
  if (!pending.value && surface?.open && (focused === surface || !surface.contains(focused)))
    cancel.value?.focus({ preventScroll: true });
}
onMounted(() => {
  dialog.value?.showModal();
  focusCancelIfNeeded();
});
watch(pending, focusCancelIfNeeded, { flush: 'post' });
const checkable = computed(
  () => status.value && ['uncertain', 'still_visible', 'unavailable'].includes(status.value),
);
const explanation = computed(() => {
  switch (status.value) {
    case 'ready':
      return '将删除你发布的这条动态。';
    case 'not_sent':
      return '删除请求没有发出。你可以重新确认。';
    case 'uncertain':
      return '尚未确认删除结果。你可以只读核对，客户端不会再次发送删除请求。';
    case 'still_visible':
      return '核对时仍能看到这条动态，先前的删除结果仍未确认。';
    case 'unavailable':
      return '目前看不到这条动态，无法据此确认删除成功。';
    case 'rejected':
      return '服务拒绝了此次删除。';
    default:
      return pending.value ? '正在核对笔记…' : '';
  }
});
function dismiss() {
  if (!pending.value) props.model.close();
}
</script>

<template>
  <dialog
    ref="dialog"
    class="community-delete-dialog"
    aria-labelledby="community-delete-title"
    @cancel.prevent="dismiss"
  >
    <h2 id="community-delete-title">确认删除动态</h2>
    <p role="status">{{ explanation }}</p>
    <p v-if="error" role="alert">{{ error }}</p>
    <div class="community-delete-actions">
      <button
        ref="cancel"
        class="outline-button"
        type="button"
        :disabled="pending"
        @click="dismiss"
      >
        {{ status === 'ready' ? '取消' : '关闭' }}
      </button>
      <button
        v-if="status === 'ready'"
        class="primary-button"
        type="button"
        :disabled="pending"
        @click="model.confirm()"
      >
        确定
      </button>
      <button
        v-else-if="checkable"
        class="outline-button"
        type="button"
        :disabled="pending"
        @click="model.check()"
      >
        核对删除结果
      </button>
      <button
        v-else-if="status === 'not_sent' || (error && !status)"
        class="outline-button"
        type="button"
        :disabled="pending"
        @click="model.inspect()"
      >
        重试读取
      </button>
    </div>
  </dialog>
</template>

<style scoped>
.community-delete-dialog {
  width: min(380px, 88vw);
  padding: 24px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--surface-raised);
  color: var(--text);
}
.community-delete-dialog::backdrop {
  background: var(--overlay);
}
.community-delete-dialog h2 {
  margin: 0;
  font-size: 19px;
}
.community-delete-dialog p {
  margin: 14px 0;
  font-size: 14px;
}
.community-delete-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
}
.community-delete-actions button {
  min-height: 34px;
  padding: 6px 14px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--surface-raised);
  color: var(--text);
}
.community-delete-actions .primary-button {
  border-color: var(--accent-fill);
  background: var(--accent-fill);
  color: var(--on-accent);
}
</style>
