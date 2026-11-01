<script setup lang="ts">
import { ref, onMounted, nextTick, watch } from 'vue';
import type { CloudModel } from '../composables/useCloud';
import { useCloudUploadForm } from '../composables/useCloudUploadForm';
import type { CloudUploadRow } from '../services/cloudUploadForm';
import Icon from './Icon.vue';
import CloudUploadFile from './cloud/CloudUploadFile.vue';
const props = defineProps<{ model: CloudModel }>();
const emit = defineEmits<{ close: []; started: [] }>();
const state = useCloudUploadForm(props.model, {
  close: () => emit('close'),
  started: () => emit('started'),
});
const dialog = ref<HTMLDialogElement>(),
  audio = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  props.model.error.value = '';
});
watch(
  () => props.model.accountEpoch.value,
  () => {
    if (!state.active() && dialog.value) dialog.value.style.visibility = 'hidden';
  },
  { flush: 'sync' },
);
async function perform(
  event: MouseEvent,
  operation: () => Promise<void> | void,
  revealError = false,
) {
  const origin = event.currentTarget as HTMLElement,
    host = dialog.value;
  if (document.activeElement === origin) host?.focus({ preventScroll: true });
  await operation();
  await nextTick();
  if (state.active() && host?.isConnected && document.activeElement === host) {
    const issue = revealError ? host.querySelector<HTMLElement>('.cloud-file-error') : null;
    const target =
      issue ??
      (origin.isConnected && !origin.matches(':disabled')
        ? origin
        : (origin.closest('nav')?.querySelector<HTMLElement>('button:not(:disabled)') ??
          host
            .querySelector<HTMLElement>('.cloud-file-error')
            ?.closest('[data-upload-file]')
            ?.querySelector<HTMLElement>('button:not(:disabled)') ??
          audio.value));
    if (issue) issue.scrollIntoView({ block: 'nearest' });
    target?.focus({ preventScroll: true });
  }
}
function choose(row: CloudUploadRow, kind: 'cover' | 'lyric' | 'cue', event: MouseEvent) {
  void perform(event, () => state.attachment(row, kind));
}
function close() {
  if (!state.blocked.value) emit('close');
}
function viewTasks() {
  if (!state.blocked.value) {
    emit('started');
    emit('close');
  }
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog cloud-upload-dialog"
    aria-labelledby="cloud-upload-title"
    tabindex="-1"
    :aria-busy="state.blocked.value"
    :data-upload-uncertain="state.uncertain.value"
    @keydown.esc.stop
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-upload-title">上传音乐</h2>
      <button
        class="icon-button"
        aria-label="关闭上传窗口"
        :disabled="state.blocked.value"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="cloud-dialog-body">
      <p class="muted">
        选择本地音频，上传后可在登录此账号的设备上收听。每个文件最大 512 MB，单次最多添加 500
        个文件。
      </p>
      <div class="cloud-source-actions">
        <button
          ref="audio"
          class="secondary"
          :disabled="!state.editable.value"
          @click="perform($event, () => state.pick('audio'))"
        >
          <Icon name="upload" />选择音频文件
        </button>
        <button
          class="secondary"
          :disabled="!state.editable.value"
          @click="perform($event, () => state.pick('folder'))"
        >
          选择文件夹
        </button>
      </div>
      <p v-if="state.message.value" class="cloud-upload-summary" role="status">
        {{ state.message.value }}
      </p>
      <p
        v-if="state.error.value || model.error.value"
        role="alert"
        class="cloud-error cloud-upload-error"
      >
        {{ state.error.value || model.error.value }}
      </p>
      <div class="cloud-file-list">
        <CloudUploadFile
          v-for="row in state.visible.value"
          :key="row.input.path"
          :row="row"
          :disabled="!state.editable.value"
          @remove="(event) => perform(event, () => state.remove(row))"
          @choose="(kind, event) => choose(row, kind, event)"
          @clear="(kind, event) => perform(event, () => state.clearAttachment(row, kind))"
        />
      </div>
      <nav
        v-if="state.rows.value.length > state.pageSize"
        class="cloud-form-pagination"
        aria-label="上传文件分页"
      >
        <button
          class="text-button"
          :disabled="!state.page.value || state.blocked.value"
          @click="
            perform($event, () => {
              state.page.value--;
            })
          "
        >
          上一页</button
        ><span
          >{{ state.page.value + 1 }} / {{ Math.ceil(state.rows.value.length / state.pageSize) }} ·
          共 {{ state.rows.value.length }} 个文件</span
        ><button
          class="text-button"
          :disabled="
            (state.page.value + 1) * state.pageSize >= state.rows.value.length ||
            state.blocked.value
          "
          @click="
            perform($event, () => {
              state.page.value++;
            })
          "
        >
          下一页
        </button>
      </nav>
      <p v-if="state.uncertain.value" class="muted">
        请保持当前文件选择后核对；已登记的任务可在“正在上传”中查看。
      </p>
    </div>
    <footer>
      <button class="secondary" :disabled="state.blocked.value" @click="close">
        {{ state.uncertain.value ? '关闭' : '取消' }}
      </button>
      <button
        v-if="state.uncertain.value"
        class="secondary"
        :disabled="state.blocked.value"
        @click="viewTasks"
      >
        查看上传任务
      </button>
      <button
        class="primary"
        :disabled="state.blocked.value || !state.rows.value.length"
        @click="perform($event, state.submit, true)"
      >
        {{
          state.submitting.value
            ? '正在添加…'
            : state.uncertain.value
              ? '核对并补全任务'
              : `开始上传${state.rows.value.length ? ` (${state.rows.value.length})` : ''}`
        }}
      </button>
    </footer>
  </dialog>
</template>
