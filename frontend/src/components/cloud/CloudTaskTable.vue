<script setup lang="ts">
import { computed, nextTick, ref, onMounted, onBeforeUnmount } from 'vue';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
import { taskProgress, uploadRowMenu } from '../../services/cloudTasks';
import {
  cloudIssue,
  cloudBytes,
  cloudTextAttachmentLabel,
  type CloudUpload,
  type CloudCommand,
} from '../../services/cloud';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';
const props = defineProps<{
  rows: CloudUpload[];
  offset: number;
  disabled: boolean;
  epoch: string;
}>();
const emit = defineEmits<{
  omit: [row: CloudUpload, origin: HTMLElement];
  remove: [row: CloudUpload, origin: HTMLElement | undefined];
  command: [command: CloudCommand, event?: MouseEvent];
  expired: [];
}>();
const client = useCore(),
  root = ref<HTMLElement>(),
  opening = ref<string>(),
  directoryError = ref('');
const titleWidth = ref(438),
  progressWidth = ref(290);
let observer: ResizeObserver | undefined;
onMounted(() => {
  const node = root.value;
  if (!node) return;
  const measure = () => {
    const growth = Math.max(0, node.clientWidth - 772) / 2;
    titleWidth.value = 438 + growth;
    progressWidth.value = 290 + growth;
  };
  observer = new ResizeObserver(measure);
  observer.observe(node);
  measure();
});
onBeforeUnmount(() => observer?.disconnect());
const menus = new Map<string, InstanceType<typeof ActionMenu>>();
function menuRef(id: string, node: unknown) {
  if (node) menus.set(id, node as InstanceType<typeof ActionMenu>);
  else menus.delete(id);
}
async function contextMenu(event: MouseEvent | KeyboardEvent, task: CloudUpload) {
  if (
    event instanceof KeyboardEvent &&
    event.key !== 'ContextMenu' &&
    !(event.shiftKey && event.key === 'F10')
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  if (props.disabled || opening.value) return;
  const origin = (event.currentTarget as HTMLElement).querySelector<HTMLElement>('.menu-trigger');
  origin?.focus({ preventScroll: true });
  const r = origin?.getBoundingClientRect(),
    pointer = event instanceof MouseEvent && (event.clientX !== 0 || event.clientY !== 0);
  await menus
    .get(task.id)
    ?.showAt(
      pointer ? event.clientX : (r?.left ?? 8),
      pointer ? event.clientY : (r?.bottom ?? 8),
      origin ?? undefined,
    );
}
const items = computed(() => uploadRowMenu(props.disabled || !!opening.value));
function progressTitle(task: CloudUpload) {
  const value = taskProgress(task);
  return task.error
    ? cloudIssue(task.error)
    : `${cloudBytes(value.bytes)} / ${cloudBytes(value.total)}` +
        (task.attachments.length ? `，包含 ${task.attachments.length} 个附件` : '');
}
const processing = (task: CloudUpload) => taskProgress(task).processing;
function remove(event: MouseEvent, row: CloudUpload) {
  if (!props.disabled) emit('remove', row, event.currentTarget as HTMLElement);
}
async function directory(task: CloudUpload) {
  if (props.disabled || opening.value) return;
  const owner = props.epoch;
  opening.value = task.id;
  directoryError.value = '';
  const origin = root.value?.querySelector<HTMLElement>(
    `[data-upload-id="${task.id}"] .menu-trigger`,
  );
  root.value?.focus({ preventScroll: true });
  try {
    await client.openCloudUploadDirectory(owner, task.id);
  } catch (error) {
    if (owner === props.epoch) {
      const issue = failure(error);
      directoryError.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
    }
  } finally {
    if (owner === props.epoch) {
      opening.value = undefined;
      await nextTick();
      if (document.activeElement === root.value && origin?.isConnected)
        origin.focus({ preventScroll: true });
    }
  }
}
</script>
<template>
  <div ref="root" class="cloud-task-table-wrap" tabindex="-1">
    <p v-if="directoryError" role="alert" class="cloud-error">{{ directoryError }}</p>
    <table class="cloud-task-table" aria-label="云盘上传任务">
      <colgroup>
        <col class="task-number-col" />
        <col class="task-title-col" :style="{ width: `${titleWidth}px` }" />
        <col class="task-progress-col" :style="{ width: `${progressWidth}px` }" />
      </colgroup>
      <thead>
        <tr>
          <th>#</th>
          <th>标题</th>
          <th>进度</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="(task, index) in rows"
          :key="task.id"
          class="cloud-upload-row"
          :data-upload-id="task.id"
          :data-upload-phase="task.phase"
          :data-upload-removing="!!task.removalPending"
          @contextmenu="contextMenu($event, task)"
          @keydown="contextMenu($event, task)"
        >
          <td class="cloud-task-number">
            <Icon v-if="task.phase === 'encoding'" name="refresh" /><span v-else>{{
              String(offset + index + 1).padStart(2, '0')
            }}</span>
          </td>
          <td>
            <div class="cloud-task-title">
              <div class="cloud-task-copy">
                <strong :title="task.title || task.fileName">{{
                  task.title || task.fileName
                }}</strong
                ><small :title="task.fileName">{{
                  task.artists?.join(' / ') || task.fileName
                }}</small>
              </div>
              <div class="cloud-task-actions">
                <button
                  v-if="task.omittableAttachment"
                  class="text-button"
                  :disabled="disabled"
                  data-testid="cloud-omit-attachment"
                  @click="emit('omit', task, $event.currentTarget as HTMLElement)"
                >
                  省略{{ cloudTextAttachmentLabel(task.omittableAttachment.attachment) }}
                </button>
                <button
                  v-if="task.canPause && !processing(task)"
                  class="icon-button"
                  :aria-label="`暂停上传 ${task.title}`"
                  title="暂停"
                  :disabled="disabled"
                  @click="emit('command', { kind: 'pause', ids: [task.id] }, $event)"
                >
                  <Icon name="pause" />
                </button>
                <button
                  v-else-if="task.canResume && !processing(task)"
                  class="icon-button"
                  :aria-label="`开始上传 ${task.title}`"
                  title="开始"
                  :disabled="disabled"
                  @click="
                    emit(
                      'command',
                      { kind: 'resume', ids: [task.id], retry_rejected: true },
                      $event,
                    )
                  "
                >
                  <Icon name="upload" />
                </button>
                <button
                  class="icon-button"
                  :aria-label="`删除上传任务 ${task.title}`"
                  :title="task.canRemove ? '删除' : '请等待当前操作完成或先核对结果'"
                  :disabled="disabled || !task.canRemove"
                  @click="remove($event, task)"
                >
                  <Icon name="trash" />
                </button>
                <ActionMenu
                  :ref="(node) => menuRef(task.id, node)"
                  :label="`上传任务更多操作 ${task.title}`"
                  :items="items"
                  :disabled="disabled || !!opening"
                  @select="directory(task)"
                />
              </div>
            </div>
          </td>
          <td>
            <div class="cloud-task-progress" :title="progressTitle(task)">
              <progress
                v-if="taskProgress(task).showProgress"
                :value="taskProgress(task).bytes"
                :max="taskProgress(task).total || 1"
                :aria-label="`${task.title} 上传进度`"
              />
              <span :class="{ failed: !!task.error }">{{ taskProgress(task).label }}</span>
              <button
                v-if="task.canRetryEncoding"
                class="text-button"
                :disabled="disabled"
                @click="emit('command', { kind: 'retry_encoding', id: task.id }, $event)"
              >
                重新检查
              </button>
              <button
                v-else-if="processing(task) && task.canResume"
                class="text-button"
                :disabled="disabled"
                @click="
                  emit('command', { kind: 'resume', ids: [task.id], retry_rejected: true }, $event)
                "
              >
                继续
              </button>
              <button
                v-else-if="task.canReconcile"
                class="text-button"
                :disabled="disabled"
                @click="emit('command', { kind: 'reconcile_upload', id: task.id }, $event)"
              >
                核对结果
              </button>
              <button
                v-else-if="task.canRetryRemove"
                class="text-button"
                :disabled="disabled"
                @click="remove($event, task)"
              >
                继续清理
              </button>
              <button
                v-else-if="task.error && task.canResume"
                class="text-button"
                :disabled="disabled"
                @click="
                  emit('command', { kind: 'resume', ids: [task.id], retry_rejected: true }, $event)
                "
              >
                重试
              </button>
              <button
                v-if="task.error && task.canRenew"
                class="text-button"
                :disabled="disabled"
                @click="emit('command', { kind: 'renew', id: task.id }, $event)"
              >
                更新上传凭证
              </button>
              <span
                v-if="processing(task) && !task.error"
                class="cloud-task-info"
                title="转码成功后可播放"
                ><Icon name="info"
              /></span>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
