<script setup lang="ts">
import { onMounted, ref, watch, nextTick } from 'vue';
import type { CloudModel } from '../../composables/useCloud';
import { useCloudTaskRemoval } from '../../composables/useCloudTaskRemoval';
import type { CloudTaskTarget } from '../../services/cloudTasks';
import { useCore } from '../../services/core';
import { cloudIssue } from '../../services/cloud';
import Icon from '../Icon.vue';
const props = defineProps<{ model: CloudModel; target: CloudTaskTarget }>(),
  emit = defineEmits<{ close: [] }>();
const state = useCloudTaskRemoval(props.model, useCore(), props.target, () => emit('close')),
  dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
watch(
  () => props.model.accountEpoch.value,
  () => {
    if (!state.active() && dialog.value) dialog.value.style.visibility = 'hidden';
  },
  { flush: 'sync' },
);
watch(state.working, async (value) => {
  const host = dialog.value;
  if (!host) return;
  if (value && host.contains(document.activeElement)) host.focus({ preventScroll: true });
  else if (!value) {
    await nextTick();
    if (state.active() && host.isConnected && document.activeElement === host)
      cancel.value?.focus({ preventScroll: true });
  }
});
function close() {
  if (!state.blocked.value) {
    if (state.sent.value && state.removed.value < props.target.ids.length)
      props.model.notice.value = state.removed.value
        ? `已移除 ${state.removed.value} 个任务，其余任务仍保留`
        : '';
    emit('close');
  }
}
const status = (id: string) => {
  const item = state.items.value.find((item) => item.id === id);
  return item?.removed
    ? '已移除'
    : (state.failures.value[id] ??
        (item?.error ? cloudIssue(item.error) : item?.requested ? '等待移除完成' : '尚未移除'));
};
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog cloud-delete-dialog cloud-task-remove-dialog"
    aria-labelledby="cloud-task-remove-title"
    :aria-busy="state.working.value"
    :data-task-remove-sent="state.sent.value"
    tabindex="-1"
    @keydown.esc.stop
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-task-remove-title">
        {{
          state.sent.value
            ? '上传任务移除结果'
            : target.history
              ? '确定清除上传记录？'
              : target.all
                ? '确定清空全部上传任务？'
                : '确定将该歌曲从列表中删除？'
        }}
      </h2>
      <button
        class="icon-button"
        aria-label="关闭任务移除确认"
        :disabled="state.blocked.value"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="cloud-dialog-body">
      <p>
        {{
          target.history ? '仅清除上传记录。' : '未完成的上传将停止，并从任务列表移除。'
        }}本地音频和已上传的云盘歌曲均保留。
      </p>
      <p v-if="target.all">
        本次范围已固定为 {{ target.ids.length }} 个任务，确认后新添加的任务会保留。
      </p>
      <p v-if="state.sent.value" role="status">
        已移除 {{ state.removed.value }} /
        {{ target.ids.length }} 个任务。未确认的任务仍保留核对入口。
      </p>
      <ul class="cloud-delete-targets">
        <li v-for="(title, index) in target.titles.slice(0, 20)" :key="target.ids[index]">
          {{ title }}<span v-if="state.sent.value"> · {{ status(target.ids[index]!) }}</span>
        </li>
        <li v-if="target.ids.length > 20">另有 {{ target.ids.length - 20 }} 个任务</li>
      </ul>
      <p v-if="state.error.value" role="alert" class="cloud-error">{{ state.error.value }}</p>
    </div>
    <footer>
      <button ref="cancel" class="secondary" :disabled="state.blocked.value" @click="close">
        {{ state.sent.value ? '返回任务列表' : '取消' }}</button
      ><button
        v-if="state.sent.value"
        class="secondary"
        :disabled="state.blocked.value"
        @click="state.read"
      >
        只读核对</button
      ><button class="danger" :disabled="state.blocked.value" @click="state.send">
        {{
          state.working.value
            ? '正在处理…'
            : state.sent.value
              ? '重试未移除任务'
              : target.history
                ? '清除记录'
                : target.all
                  ? '清空上传任务'
                  : '删除任务'
        }}
      </button>
    </footer>
  </dialog>
</template>
