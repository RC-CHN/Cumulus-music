<script setup lang="ts">
import { ref, onMounted, watch, nextTick } from 'vue';
import type { CloudModel } from '../composables/useCloud';
import { useCloudDelete, type CloudDeleteTarget } from '../composables/useCloudDelete';
import { deletionMessage } from '../services/cloud';
import { cloudDeleteWarning } from '../services/cloudDelete';
import CloudDeleteTargets from './cloud/CloudDeleteTargets.vue';
import CloudDeleteLocal from './cloud/CloudDeleteLocal.vue';
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
  void state.prepare();
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
    :data-cloud-delete-ready="!!state.removal.value"
    :aria-busy="state.working.value || state.preparing.value"
    @keydown.esc.stop
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-delete-title">
        {{
          state.working.value && state.activity.value === 'send'
            ? `正在删除 ${target.ids.length} 首歌曲`
            : state.submitted.value ||
                state.operation.value ||
                (state.unknown.value && (!state.working.value || state.activity.value === 'read'))
              ? '云盘删除结果'
              : `从云盘删除 ${target.ids.length} 首歌曲？`
        }}
      </h2>
      <button
        class="icon-button"
        :aria-label="state.submitted.value ? '关闭删除结果' : '关闭删除确认'"
        :disabled="state.working.value"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="cloud-dialog-body">
      <p v-if="state.resumed.value" class="cloud-delete-local-note">
        以下为此前确认的删除范围和本地文件选项。
      </p>
      <p
        v-if="!state.submitted.value && !state.operation.value && !state.unknown.value"
        class="cloud-delete-warning"
      >
        {{ cloudDeleteWarning(target) }}
      </p>
      <p
        v-else-if="state.removal.value?.error && !state.unknown.value"
        class="cloud-error cloud-delete-result"
        role="status"
      >
        {{ state.removal.value.error }}
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
      <p v-if="state.preparing.value" role="status">正在检查本地关联…</p>
      <CloudDeleteLocal
        v-model="state.deleteLocal.value"
        :view="state.removal.value"
        :total="target.ids.length"
        :disabled="state.working.value || state.submitted.value || !!state.removal.value?.running"
        :submitted="state.submitted.value"
      />
      <CloudDeleteTargets
        :target="target"
        :disabled="state.working.value"
        :submitted="state.submitted.value || !!state.operation.value"
      />
      <p
        v-if="
          state.stale.value &&
          !state.submitted.value &&
          !state.operation.value &&
          !state.unknown.value
        "
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
          state.submitted.value ||
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
        v-else-if="state.working.value && state.activity.value === 'send'"
        class="primary"
        disabled
      >
        正在删除…
      </button>
      <button v-else-if="state.canContinue.value" class="primary" @click="state.resume()">
        {{ state.removal.value?.phase === 'result' ? '重试本地清理' : '继续删除' }}
      </button>
      <button
        v-else-if="!state.removal.value && !state.submitted.value"
        class="primary"
        :disabled="!state.canPrepare.value"
        @click="state.prepare()"
      >
        {{ state.preparing.value ? '正在检查…' : '重新检查' }}
      </button>
      <button
        v-else-if="!state.submitted.value"
        class="primary"
        :disabled="!state.canSend.value"
        @click="state.send()"
      >
        {{ state.working.value ? '正在删除…' : '删除歌曲' }}
      </button>
    </footer>
  </dialog>
</template>
