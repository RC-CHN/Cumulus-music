<script setup lang="ts">
import { useMotion } from '../composables/useMotion';
import { computed, nextTick, ref, watch } from 'vue';
import type { CommentWritesModel } from '../composables/useCommentWrites';
import type { CommentItem } from '../services/comments';
import {
  commentWriteMessage,
  commentWritePending,
  type CommentWrite,
} from '../services/commentWrites';
import Icon from './Icon.vue';
const props = defineProps<{ model: CommentWritesModel }>();
const { draft, reply, busy, loading, pending, operations, error, message, validText } = props.model;
const input = ref<HTMLTextAreaElement>(),
  dialog = ref<HTMLDialogElement>();
const recent = computed(() => operations.value.filter((v) => !commentWritePending(v)).slice(0, 8));
const confirmation = ref<
  { kind: 'delete'; item: CommentItem } | { kind: 'resume' | 'abandon'; row: CommentWrite }
>();
watch(confirmation, async (value) => {
  await nextTick();
  if (value && confirmation.value === value) dialog.value?.showModal();
});
function requestDelete(item: CommentItem) {
  if (item.owned && !busy.value && !pending.value.length)
    confirmation.value = { kind: 'delete', item };
}
function focus() {
  input.value?.focus();
  input.value?.scrollIntoView({
    block: 'center',
    behavior: useMotion().reduced.value ? 'instant' : 'smooth',
  });
}
async function confirm() {
  const action = confirmation.value;
  if (!action || busy.value) return;
  confirmation.value = undefined;
  if (action.kind === 'delete') await props.model.remove(action.item);
  else
    await props.model.action(action.row, { kind: action.kind === 'resume' ? 'submit' : 'abandon' });
}
function resume(row: CommentWrite) {
  if (row.command.kind === 'delete') confirmation.value = { kind: 'resume', row };
  else void props.model.action(row, { kind: 'submit' });
}
function abandon(row: CommentWrite) {
  if (row.phase === 'prepared') void props.model.action(row, { kind: 'abandon' });
  else confirmation.value = { kind: 'abandon', row };
}
const label = (row: CommentWrite) =>
  row.command.kind === 'delete' ? '删除评论' : row.command.replyTo ? '回复评论' : '发布评论';
defineExpose({ requestDelete, focus });
</script>
<template>
  <section class="comment-write-panel" aria-label="发表或回复评论">
    <div v-if="reply" class="comment-reply-draft">
      <span>回复 {{ reply.author?.name || '云音乐用户' }}：{{ reply.content }}</span>
      <button class="text-button" :disabled="busy || !!pending.length" @click="model.setReply()">
        取消回复
      </button>
    </div>
    <form @submit.prevent="model.publish">
      <textarea
        ref="input"
        v-model="draft"
        :disabled="busy || !!pending.length"
        :placeholder="reply ? '写下你的回复' : '说点什么吧'"
        rows="4"
        aria-label="评论内容"
        @keydown.ctrl.enter.prevent="model.publish"
        @keydown.meta.enter.prevent="model.publish"
      ></textarea>
      <div class="comment-compose-footer">
        <span :class="{ overlength: draft.length > 1000 }" aria-live="polite"
          >{{ draft.length }} / 1000</span
        ><button class="primary" :disabled="busy || loading || !!pending.length || !validText">
          {{ busy ? '正在处理…' : reply ? '发布回复' : '发布评论' }}
        </button>
      </div>
    </form>
    <p v-if="error" class="error-state" role="alert">{{ error }}</p>
    <p v-if="message" class="comment-write-message" role="status">{{ message }}</p>
    <section v-if="pending.length" class="comment-write-pending" aria-label="待确认的评论操作">
      <article v-for="row in pending" :key="row.id" :data-write-phase="row.phase">
        <strong>{{ label(row) }}</strong>
        <p>{{ commentWriteMessage(row) }}</p>
        <p v-if="row.command.kind === 'publish'" class="comment-write-excerpt">
          {{ row.command.content }}
        </p>
        <button
          v-if="row.phase === 'prepared'"
          class="text-button"
          :disabled="busy"
          @click="resume(row)"
        >
          {{ row.command.kind === 'delete' ? '继续删除' : '继续发布' }}
        </button>
        <button
          v-else
          class="text-button"
          :disabled="busy"
          @click="model.action(row, { kind: 'reconcile' })"
        >
          核对提交结果
        </button>
        <button class="text-button" :disabled="busy" @click="abandon(row)">
          {{ row.phase === 'prepared' ? '取消未发送操作' : '停止核对' }}
        </button>
      </article>
    </section>
    <details v-if="recent.length" class="comment-write-history">
      <summary>近期评论操作</summary>
      <article v-for="row in recent" :key="row.id" :data-write-phase="row.phase">
        <strong>{{ label(row) }}</strong
        ><span>{{ commentWriteMessage(row) }}</span
        ><button
          v-if="row.command.kind === 'publish' && row.phase !== 'confirmed'"
          class="text-button"
          :disabled="busy || !!pending.length"
          @click="
            model.reuse(row);
            focus();
          "
        >
          取回内容
        </button>
      </article>
    </details>
    <button
      v-if="error || pending.length"
      class="text-button"
      :disabled="busy || loading"
      @click="model.refresh"
    >
      刷新操作记录
    </button>
    <dialog
      v-if="confirmation"
      ref="dialog"
      class="comment-write-confirm"
      aria-labelledby="comment-write-confirm-title"
      @cancel.prevent="confirmation = undefined"
    >
      <header>
        <h2 id="comment-write-confirm-title">
          {{ confirmation.kind === 'abandon' ? '停止核对这次操作？' : '删除这条评论？' }}
        </h2>
        <button class="icon-button" aria-label="关闭评论确认" @click="confirmation = undefined">
          <Icon name="close" />
        </button>
      </header>
      <p v-if="confirmation.kind === 'abandon'">
        停止核对不会撤回可能已提交的内容，也不表示操作失败。你仍可在评论页查看结果。
      </p>
      <template v-else
        ><p>删除后将不再公开显示这条评论。</p>
        <blockquote>
          {{
            confirmation.kind === 'delete'
              ? confirmation.item.content
              : confirmation.row.target?.content
          }}
        </blockquote></template
      >
      <footer>
        <button class="download-button" @click="confirmation = undefined">取消</button
        ><button class="primary" @click="confirm">
          {{ confirmation.kind === 'abandon' ? '停止核对' : '确认删除' }}
        </button>
      </footer>
    </dialog>
  </section>
</template>
<style scoped>
.comment-write-panel {
  margin: 20px 0 28px;
}
.comment-write-panel textarea {
  width: 100%;
  min-height: 100px;
  resize: vertical;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-raised);
  padding: 12px;
  line-height: 22px;
  color: inherit;
  font: inherit;
}
.comment-compose-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 18px;
  margin-top: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}
.comment-compose-footer .primary {
  min-width: 175px;
  height: 40px;
  border-radius: 20px;
}
.overlength {
  color: var(--accent);
}
.comment-reply-draft {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
  background: var(--surface-alt);
  padding: 10px 12px;
  margin-bottom: 10px;
  border-radius: 8px;
  font-size: 13px;
}
.comment-reply-draft span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.comment-write-pending {
  background: var(--surface-alt);
  padding: 12px;
  border-radius: 8px;
  margin-top: 16px;
  font-size: 13px;
}
.comment-write-pending button {
  margin-right: 16px;
}
.comment-write-excerpt {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 90px;
  overflow-y: auto;
  color: var(--text-secondary);
}
.comment-write-history {
  font-size: 12px;
  margin: 12px 0;
  color: var(--text-secondary);
}
.comment-write-history summary {
  cursor: pointer;
}
.comment-write-history article {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  padding: 8px 0;
}
.comment-write-message {
  font-size: 13px;
  color: var(--text-secondary);
}
.comment-write-confirm {
  width: 440px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 32px);
  padding: 24px;
  border: 0;
  border-radius: 16px;
  color: var(--text);
  background: var(--surface-raised);
}
.comment-write-confirm::backdrop {
  background: #0005;
}
.comment-write-confirm header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.comment-write-confirm h2 {
  font-size: 18px;
}
.comment-write-confirm p,
.comment-write-confirm blockquote {
  font-size: 14px;
  line-height: 23px;
  overflow-wrap: anywhere;
}
.comment-write-confirm blockquote {
  margin: 14px 0;
  max-height: 150px;
  overflow: auto;
  color: var(--text-secondary);
  border-left: 2px solid var(--line);
  padding-left: 12px;
  white-space: pre-wrap;
}
.comment-write-confirm footer {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  margin-top: 24px;
}
</style>
