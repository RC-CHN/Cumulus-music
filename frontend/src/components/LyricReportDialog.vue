<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import type { useLyricReport } from '../composables/useLyricReport';
import Icon from './Icon.vue';
const props = defineProps<{ model: ReturnType<typeof useLyricReport> }>();
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
watch([props.model.working, props.model.reading], async ([working, reading]) => {
  const node = dialog.value;
  if (!node) return;
  if (working || reading) {
    if (node.contains(document.activeElement)) node.focus({ preventScroll: true });
  } else {
    await nextTick();
    if (
      node.isConnected &&
      !node.inert &&
      (document.activeElement === node || document.activeElement === document.body)
    )
      cancel.value?.focus({ preventScroll: true });
  }
});
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog lyric-report-dialog"
    :data-report-id="model.record.value?.id"
    :data-report-phase="model.record.value?.phase"
    :data-report-working="model.working.value"
    :data-report-loading="model.reading.value"
    aria-labelledby="lyric-report-title"
    :aria-busy="model.working.value || model.reading.value"
    @keydown.esc.stop
    @cancel.prevent="model.close()"
  >
    <header>
      <h2 id="lyric-report-title">歌词报错</h2>
      <button
        class="icon-button"
        aria-label="关闭歌词报错"
        :disabled="model.working.value"
        @click="model.close()"
      >
        <Icon name="close" />
      </button>
    </header>
    <form @submit.prevent="model.submit()">
      <div class="lyric-report-body">
        <p v-if="model.record.value" class="lyric-report-target">
          针对歌曲「{{ model.record.value.title }}」
        </p>
        <p v-if="model.reading.value" class="muted" role="status">正在读取报错信息…</p>
        <p v-if="model.issue.value" class="form-error" role="alert">
          {{ model.issue.value
          }}<button
            type="button"
            class="text-button"
            :disabled="model.working.value || model.reading.value"
            @click="model.refresh()"
          >
            重新读取状态
          </button>
        </p>
        <template v-if="model.record.value">
          <template v-if="model.uncertain.value">
            <p class="form-warning" role="status">
              发送结果尚未确认，已保留描述和记录。暂时无法核实是否送达，请勿重复发送。
            </p>
            <blockquote>{{ model.content.value }}</blockquote>
            <button
              type="button"
              class="text-button"
              :disabled="model.working.value || model.reading.value"
              @click="model.refresh()"
            >
              重新读取状态
            </button>
            <p class="muted">停止本地跟踪只保留记录并解除本地限制，不会撤回服务端反馈。</p>
            <button
              type="button"
              class="text-button danger"
              :disabled="model.working.value || model.reading.value"
              @click="model.abandon()"
            >
              停止本地跟踪
            </button>
          </template>
          <template v-else>
            <fieldset :disabled="model.working.value || model.reading.value">
              <legend>错误类型 <span class="muted">（必填）</span></legend>
              <div class="lyric-report-types" aria-label="错误类型" role="group">
                <button
                  v-for="option in model.record.value.options"
                  :key="option.key"
                  type="button"
                  :data-report-type="option.key"
                  :aria-pressed="model.selected.value === option.key"
                  :class="{ selected: model.selected.value === option.key }"
                  @click="model.selected.value = option.key"
                >
                  {{ option.label }}
                </button>
              </div>
              <p v-if="model.typeIssue.value" class="form-error" role="alert">
                {{ model.typeIssue.value }}
              </p>
              <p v-else-if="model.loaded.value && !model.record.value.options.length" class="muted">
                暂时没有可用的错误类型
              </p>
              <button
                v-if="
                  model.typeIssue.value ||
                  (model.loaded.value && !model.record.value.options.length)
                "
                type="button"
                class="text-button"
                @click="model.types()"
              >
                重新读取错误类型
              </button>
            </fieldset>
            <label class="lyric-report-description" for="lyric-report-content"
              >错误描述 <span class="muted">（必填）</span></label
            >
            <textarea
              id="lyric-report-content"
              v-model="model.content.value"
              rows="2"
              maxlength="50"
              placeholder="请在此处添加描述（5–50字）"
              :disabled="model.working.value || model.reading.value"
              aria-describedby="lyric-report-count"
            />
            <p id="lyric-report-count" class="lyric-report-count muted">
              {{ model.content.value.length }} / 50
            </p>
            <p v-if="model.record.value.phase === 'rejected'" class="form-error" role="alert">
              {{
                model.record.value.issue === 'not_sent'
                  ? '请求准备未完成，反馈尚未发送；请稍后重试。'
                  : '发送失败，已保留错误类型和描述。'
              }}
            </p>
          </template>
        </template>
      </div>
      <footer>
        <button
          ref="cancel"
          type="button"
          class="outline"
          :disabled="model.working.value"
          @click="model.close()"
        >
          {{ model.uncertain.value ? '保留记录并关闭' : '取消' }}</button
        ><button
          v-if="!model.uncertain.value"
          type="submit"
          class="primary"
          :disabled="!model.canSubmit.value"
        >
          {{ model.working.value ? '正在发送…' : '发送' }}
        </button>
      </footer>
    </form>
  </dialog>
</template>
<style scoped>
.lyric-report-dialog {
  width: 574px;
  max-width: calc(100vw - 32px);
  max-height: calc(100dvh - 48px);
  overflow: hidden;
}
.lyric-report-dialog:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.lyric-report-dialog form {
  display: flex;
  min-height: 0;
  flex-direction: column;
  max-height: calc(100dvh - 132px);
}
.lyric-report-body {
  padding: 20px 28px 4px;
  overflow: auto;
  min-height: 0;
  overflow-wrap: anywhere;
}
.lyric-report-target {
  margin: 0 0 20px;
  color: var(--muted);
}
fieldset {
  border: 0;
  margin: 0 0 22px;
  padding: 0;
  min-width: 0;
}
legend,
.lyric-report-description {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 12px;
}
.lyric-report-types {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 12px;
}
.lyric-report-types button {
  padding: 7px 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font-size: 13px;
  white-space: normal;
  text-align: left;
}
.lyric-report-types button.selected {
  border-color: var(--accent);
  background: var(--accent-soft);
  color: var(--accent);
}
.lyric-report-description {
  display: block;
}
textarea {
  width: 100%;
  resize: vertical;
  min-height: 72px;
  max-height: 160px;
  box-sizing: border-box;
  line-height: 1.6;
  scroll-margin-block: 8px 32px;
}
textarea:focus-visible {
  outline-offset: -2px;
}
.lyric-report-count {
  text-align: right;
  margin: 6px 0 0;
  font-size: 12px;
}
blockquote {
  margin: 16px 0;
  padding: 12px;
  background: var(--surface-hover);
  border-radius: 6px;
}
.danger {
  color: var(--accent);
}
footer {
  flex-shrink: 0;
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding: 22px 28px;
}
@media (max-width: 600px) {
  .lyric-report-body {
    padding-inline: 20px;
  }
  footer {
    padding-inline: 20px;
  }
}
</style>
