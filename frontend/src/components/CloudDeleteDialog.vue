<script setup lang="ts">
import { ref, onMounted, watch, nextTick } from 'vue';
import type { CloudModel } from '../composables/useCloud';
import { useCloudDelete, type CloudDeleteTarget } from '../composables/useCloudDelete';
import { deletionMessage } from '../services/cloud';
import { cloudDeleteWarning } from '../services/cloudDelete';
import CloudDeleteTargets from './cloud/CloudDeleteTargets.vue';
import Icon from './Icon.vue';
const props = defineProps<{ model: CloudModel; target: CloudDeleteTarget }>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
const state = useCloudDelete(props.model, props.target, () => emit('close'));
const target = state.target;
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
  props.model.error.value = '';
});
watch(state.working, async (value) => {
  const node = dialog.value;
  if (!node) return;
  if (value && node.contains(document.activeElement)) node.focus({ preventScroll: true });
  else if (!value) {
    await nextTick();
    if (node.isConnected && !node.inert && document.activeElement === node) cancel.value?.focus();
  }
});
function close() {
  if (!state.working.value) emit('close');
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog cloud-delete-dialog"
    aria-labelledby="cloud-delete-title"
    :data-cloud-delete-phase="state.operation.value?.phase"
    :data-cloud-delete-working="state.working.value"
    :aria-busy="state.working.value"
    @keydown.esc.stop
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-delete-title">
        {{
          state.working.value && state.activity.value === 'send'
            ? `正在删除 ${target.ids.length} 首歌曲`
            : state.operation.value ||
                (state.unknown.value && (!state.working.value || state.activity.value === 'read'))
              ? '云盘删除结果'
              : `从云盘删除 ${target.ids.length} 首歌曲？`
        }}
      </h2>
      <button
        class="icon-button"
        aria-label="关闭删除确认"
        :disabled="state.working.value"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="cloud-dialog-body">
      <p v-if="!state.operation.value && !state.unknown.value" class="cloud-delete-warning">
        {{ cloudDeleteWarning(target) }}
      </p>
      <p v-else-if="state.operation.value" role="status" class="cloud-delete-result">
        {{ deletionMessage(state.operation.value)
        }}<template v-if="state.operation.value.total !== target.ids.length"
          >（原操作共 {{ state.operation.value.total }} 首）</template
        >
      </p>
      <p v-else-if="state.working.value" role="status">
        {{ state.activity.value === 'read' ? '正在核对删除结果…' : '正在删除，请稍候…' }}
      </p>
      <p v-else class="cloud-error" role="status">
        删除结果尚未确认。请只读核对，暂时不要重复删除。
      </p>
      <p class="cloud-delete-local-note">操作范围为当前账号的云盘资源，本地文件保留。</p>
      <CloudDeleteTargets
        :target="target"
        :disabled="state.working.value"
        :submitted="state.unknown.value || !!state.operation.value"
      />
      <p
        v-if="state.stale.value && !state.operation.value && !state.unknown.value"
        class="cloud-error"
        role="status"
      >
        云盘列表已变化，请返回列表重新选择歌曲。
      </p>
      <p v-if="model.error.value || model.readError.value" role="alert" class="cloud-error">
        {{ model.error.value || model.readError.value }}
      </p>
    </div>
    <footer>
      <button ref="cancel" class="secondary" :disabled="state.working.value" @click="close">
        {{
          state.operation.value ||
          (state.unknown.value && (!state.working.value || state.activity.value === 'read'))
            ? '保留记录并返回'
            : '保留歌曲'
        }}
      </button>
      <button
        v-if="state.needsRead.value && (!state.working.value || state.activity.value === 'read')"
        class="primary"
        :disabled="!state.canRead.value"
        @click="state.read()"
      >
        {{ state.working.value ? '正在核对…' : '只读核对' }}
      </button>
      <button
        v-else-if="!state.operation.value"
        class="primary"
        :disabled="!state.canSend.value"
        @click="state.send()"
      >
        {{ state.working.value ? '正在删除…' : '删除歌曲' }}
      </button>
    </footer>
  </dialog>
</template>
