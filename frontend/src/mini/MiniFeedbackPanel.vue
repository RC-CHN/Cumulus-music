<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import type { useMiniFeedback } from '../composables/useMiniFeedback';
import Icon from '../components/Icon.vue';
const props = defineProps<{ model: ReturnType<typeof useMiniFeedback>; navigationIssue: string }>();
const emit = defineEmits<{ main: [] }>();
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
watch(
  [props.model.writing, props.model.reading, props.model.loadingReasons, props.model.needsRead],
  async () => {
    const element = dialog.value;
    if (!element || !element.contains(document.activeElement)) return;
    element.focus({ preventScroll: true });
    await nextTick();
    if (
      !element.isConnected ||
      element.inert ||
      document.activeElement !== element ||
      props.model.writing.value ||
      props.model.reading.value ||
      props.model.loadingReasons.value
    )
      return;
    const target =
      element.querySelector<HTMLElement>('.mini-feedback-read:not(:disabled)') ?? cancel.value;
    target?.focus({ preventScroll: true });
    target?.scrollIntoView({ block: 'nearest' });
  },
);
</script>
<template>
  <dialog
    ref="dialog"
    class="mini-feedback"
    :data-entry-id="model.target.value?.entryId"
    :data-needs-read="model.needsRead.value"
    :data-review="model.state.value?.pending"
    aria-labelledby="mini-feedback-title"
    :aria-busy="model.writing.value || model.reading.value"
    @keydown.esc.stop
    @cancel.prevent="model.close"
  >
    <header>
      <h2 id="mini-feedback-title" :title="model.target.value?.title">
        减少推荐 · {{ model.target.value?.title }}
      </h2>
      <button aria-label="关闭减少推荐" :disabled="model.writing.value" @click="model.close">
        <Icon name="close" />
      </button>
    </header>
    <div class="mini-feedback-body">
      <p>减少此歌曲的推荐，并播放下一首。</p>
      <p v-if="!model.sameEntry.value" class="mini-feedback-warning" role="status">
        当前歌曲已切换，请关闭后重新打开。
      </p>
      <p
        v-else-if="model.changed.value && !model.writing.value"
        class="mini-feedback-warning"
        role="status"
      >
        播放状态已更新，请先重新读取。
      </p>
      <p v-if="model.state.value?.pending" class="mini-feedback-warning" role="alert">
        有反馈尚待核对，请到主窗口确认，避免重复提交。
      </p>
      <p v-if="model.issue.value" class="mini-feedback-warning" role="alert">
        {{ model.issue.value.message }}
      </p>
      <p v-if="model.readIssue.value" class="mini-feedback-warning" role="alert">
        状态读取：{{ model.readIssue.value.message }}
      </p>
      <div v-if="model.sameEntry.value && model.needsRead.value" class="mini-feedback-recovery">
        <button
          class="mini-feedback-read"
          :disabled="model.writing.value || model.reading.value"
          @click="model.readState()"
        >
          {{ model.reading.value ? '正在读取状态…' : '重新读取状态' }}
        </button>
        <span>只读查询，不会重复提交。</span>
      </div>
      <p v-else-if="model.reading.value" role="status">正在读取反馈状态…</p>
      <p v-if="model.loadingReasons.value" role="status">正在读取反馈选项…</p>
      <p v-if="model.reasonIssue.value" class="mini-feedback-warning" role="alert">
        {{ model.reasonIssue.value.message }}
        <button
          :disabled="!model.canSubmit.value || model.loadingReasons.value"
          @click="model.loadReasons"
        >
          重新加载选项
        </button>
      </p>
      <div v-if="model.reasons.value" class="mini-feedback-reasons" aria-label="反馈选项">
        <button
          v-for="reason in [...model.reasons.value.options].reverse()"
          :key="reason.key"
          :data-reason-key="reason.key"
          :data-artist-id="reason.artistId"
          :disabled="!model.canSubmit.value"
          :title="reason.label"
          @click="model.submit(reason.key)"
        >
          {{ reason.label }}
        </button>
        <p v-if="!model.reasons.value.options.length">暂无更多反馈选项，仍可仅减少此歌曲。</p>
      </div>
      <p v-if="navigationIssue" class="mini-feedback-warning" role="alert">{{ navigationIssue }}</p>
    </div>
    <footer>
      <button ref="cancel" :disabled="model.writing.value" @click="model.close">取消</button>
      <button
        v-if="model.issue.value || model.state.value?.pending || model.readIssue.value"
        :disabled="model.writing.value"
        @click="emit('main')"
      >
        到主窗口查看
      </button>
      <button
        class="mini-feedback-submit"
        :disabled="!model.canSubmit.value"
        @click="model.submit()"
      >
        {{ model.writing.value ? '正在提交…' : '仅减少此歌曲' }}
      </button>
    </footer>
  </dialog>
</template>
<style scoped>
.mini-feedback {
  width: calc(100vw - 16px);
  max-width: 410px;
  max-height: calc(100dvh - 16px);
  box-sizing: border-box;
  margin: auto;
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 0;
  background: var(--surface);
  color: var(--text);
  box-shadow: 0 8px 28px #0003;
  overflow: hidden;
}
.mini-feedback[open] {
  display: flex;
  flex-direction: column;
}
.mini-feedback::backdrop {
  background: var(--overlay);
}
.mini-feedback header,
.mini-feedback footer {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: var(--surface);
}
.mini-feedback header {
  border-bottom: 1px solid var(--line);
}
.mini-feedback h2 {
  font-size: 12px;
  margin: 0;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.mini-feedback header button {
  width: 24px;
  height: 24px;
}
.mini-feedback-body {
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  scroll-padding: 4px;
  padding: 0 10px;
}
.mini-feedback-body p {
  font-size: 11px;
  line-height: 1.5;
  margin: 8px 0;
  overflow-wrap: anywhere;
}
.mini-feedback-warning {
  color: var(--accent);
}
.mini-feedback footer {
  justify-content: flex-end;
  border-top: 1px solid var(--line);
}
.mini-feedback footer button {
  min-height: 28px;
  padding: 4px 8px;
  font-size: 11px;
}
.mini-feedback footer .mini-feedback-submit {
  background: var(--accent);
  color: white;
}
.mini-feedback-reasons {
  display: grid;
  gap: 3px;
  padding-bottom: 6px;
}
.mini-feedback-reasons button {
  display: block;
  width: 100%;
  min-height: 30px;
  padding: 5px 8px;
  text-align: left;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  background: var(--surface-hover);
  color: var(--text);
}
.mini-feedback-recovery {
  display: grid;
  gap: 4px;
  font-size: 10px;
  margin: 8px 0;
}
.mini-feedback-recovery button {
  justify-self: start;
  color: var(--accent);
}
.mini-feedback-recovery span {
  color: var(--muted);
}
.mini-feedback:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
</style>
