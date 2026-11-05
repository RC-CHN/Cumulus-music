<script setup lang="ts">
import { nextTick, onMounted, ref, useId } from 'vue';
import type { VinylComposerOpening } from '../composables/useVinylCommentComposer';
import CommentWritePanel from './CommentWritePanel.vue';
import Icon from './Icon.vue';

const props = defineProps<{ opening: VinylComposerOpening }>();
const emit = defineEmits<{
  close: [opening: VinylComposerOpening];
  independent: [opening: VinylComposerOpening];
}>();
const titleId = useId();
const dialog = ref<HTMLDialogElement>();
const editor = ref<InstanceType<typeof CommentWritePanel>>();
onMounted(async () => {
  const host = dialog.value;
  host?.showModal();
  await nextTick();
  if (host?.isConnected && host.open && !host.inert) editor.value?.focus();
});
</script>

<template>
  <dialog
    ref="dialog"
    class="vinyl-comment-dialog"
    :aria-labelledby="titleId"
    @cancel.prevent="emit('close', props.opening)"
  >
    <header>
      <h2 :id="titleId" :title="opening.target.title">
        {{ opening.reply ? '回复评论' : `歌曲：${opening.target.title}` }}
      </h2>
      <button
        type="button"
        class="icon-button"
        aria-label="关闭评论编辑"
        @click="emit('close', opening)"
      >
        <Icon name="close" />
      </button>
    </header>
    <CommentWritePanel
      ref="editor"
      :model="opening.writer"
      vinyl
      @independent="emit('independent', opening)"
    />
  </dialog>
</template>

<style scoped>
.vinyl-comment-dialog {
  box-sizing: border-box;
  width: min(534px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  margin: auto;
  padding: 16px 20px;
  overflow: auto;
  border: 0;
  border-radius: 12px;
  background: var(--surface);
  color: var(--text);
  box-shadow: 0 24px 90px #24132833;
}
.vinyl-comment-dialog::backdrop {
  background: var(--overlay);
}
.vinyl-comment-dialog header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.vinyl-comment-dialog h2 {
  margin: 0;
  font-size: 16px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
