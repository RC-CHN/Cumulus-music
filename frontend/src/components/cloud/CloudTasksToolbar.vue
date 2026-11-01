<script setup lang="ts">
import { ref, nextTick, onScopeDispose } from 'vue';
import type { CloudModel } from '../../composables/useCloud';
import { cloudTaskTarget, type CloudTaskTarget } from '../../services/cloudTasks';
import { useCore } from '../../services/core';
import { failure } from '../../services/contracts';
import Icon from '../Icon.vue';
const props = defineProps<{
  model: CloudModel;
  disabled: boolean;
  full: boolean;
  history: boolean;
}>();
const emit = defineEmits<{
  remove: [target: CloudTaskTarget, origin: HTMLElement | undefined];
  expired: [];
}>();
const client = useCore(),
  root = ref<HTMLElement>(),
  reading = ref(false),
  error = ref('');
let alive = true;
const active = (owner: string) => alive && props.model.accountEpoch.value === owner;
onScopeDispose(() => {
  alive = false;
});
async function perform(event: MouseEvent, action: () => Promise<unknown>) {
  const origin = event.currentTarget as HTMLElement,
    owner = props.model.accountEpoch.value;
  root.value?.focus({ preventScroll: true });
  await action();
  await nextTick();
  if (
    active(owner) &&
    document.activeElement === root.value &&
    origin.isConnected &&
    !origin.matches(':disabled')
  )
    origin.focus({ preventScroll: true });
}
async function clear(event: MouseEvent) {
  if (props.disabled || reading.value) return;
  const owner = props.model.accountEpoch.value,
    history = props.history,
    origin = event.currentTarget as HTMLElement;
  reading.value = true;
  error.value = '';
  root.value?.focus({ preventScroll: true });
  try {
    const value = await client.cloudUploadSelection(owner, history);
    if (!active(owner) || history !== props.history) return;
    if (
      value.epoch !== owner ||
      value.offset !== 0 ||
      value.total !== value.uploads.length ||
      value.uploads.some(
        (row) =>
          !/^[a-f0-9]{32}$/.test(row.id) ||
          ['complete', 'cancelled'].includes(row.phase) !== history,
      ) ||
      value.total > 50000 ||
      new Set(value.uploads.map((row) => row.id)).size !== value.total
    )
      throw { code: 'unavailable', message: '上传任务范围不完整，请重新读取' };
    if (value.uploads.length)
      emit('remove', cloudTaskTarget(owner, value.uploads, true, history), origin);
    else error.value = '当前没有可清理的上传任务。';
  } catch (e) {
    if (active(owner)) {
      const issue = failure(e);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
    }
  } finally {
    if (active(owner)) {
      reading.value = false;
      await nextTick();
      if (document.activeElement === root.value && origin.isConnected)
        origin.focus({ preventScroll: true });
    }
  }
}
defineExpose({ reading });
</script>
<template>
  <div ref="root" class="cloud-task-toolbar" tabindex="-1" :aria-busy="reading">
    <template v-if="!history"
      ><button
        class="secondary"
        :disabled="disabled || reading || full || !model.view.value?.uploadingCount"
        @click="perform($event, () => model.command({ kind: 'resume_all', retry_rejected: true }))"
      >
        <Icon name="cloud" />全部上传</button
      ><button
        class="secondary"
        :disabled="disabled || reading || !model.view.value?.uploadingCount"
        @click="perform($event, () => model.command({ kind: 'pause_all' }))"
      >
        <Icon name="pause" />全部暂停</button
      ><button
        class="secondary"
        :disabled="disabled || reading || !model.view.value?.uploadingCount"
        @click="clear"
      >
        <Icon name="trash" />全部清空
      </button></template
    >
    <template v-else
      ><button
        class="secondary"
        :disabled="disabled || reading"
        @click="model.filter({ tab: 'uploading' })"
      >
        <Icon name="back" />返回上传</button
      ><button
        class="secondary"
        :disabled="disabled || reading || !model.page.value?.total"
        @click="clear"
      >
        <Icon name="trash" />清除记录
      </button></template
    >
    <span v-if="reading" class="muted" role="status">正在读取完整任务列表…</span>
    <p v-if="error" class="cloud-error" role="alert">
      {{ error
      }}<button class="text-button" :disabled="disabled || reading" @click="clear">重新读取</button>
    </p>
  </div>
</template>
