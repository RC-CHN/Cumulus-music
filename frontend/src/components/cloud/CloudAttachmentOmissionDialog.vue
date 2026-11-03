<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { CloudModel } from '../../composables/useCloud';
import { cloudTextAttachmentLabel, type CloudAttachmentOmission } from '../../services/cloud';

const props = defineProps<{
  model: CloudModel;
  target: { epoch: string; id: string; title: string; choice: CloudAttachmentOmission };
}>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>();
const cancel = ref<HTMLButtonElement>();
const working = ref(false);
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
function close() {
  if (!working.value) emit('close');
}
async function confirm() {
  const { epoch, id, choice } = props.target;
  if (working.value || epoch !== props.model.accountEpoch.value) return;
  working.value = true;
  dialog.value?.focus({ preventScroll: true });
  try {
    const result = await props.model.command({
      kind: 'omit_text_attachment',
      id,
      attachment: choice.attachment,
      expected_revision: choice.revision,
    });
    if (epoch === props.model.accountEpoch.value && result?.accepted.includes(id))
      props.model.notice.value = '已省略附件。可以手动继续上传。';
  } finally {
    working.value = false;
    if (epoch === props.model.accountEpoch.value) emit('close');
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog cloud-delete-dialog"
    aria-labelledby="cloud-omit-attachment-title"
    :aria-busy="working"
    tabindex="-1"
    @keydown.esc.stop
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-omit-attachment-title">
        省略{{ cloudTextAttachmentLabel(target.choice.attachment) }}？
      </h2>
    </header>
    <div class="cloud-dialog-body">
      <p>
        当前服务无法上传「{{
          target.title
        }}」的这项附件。省略只影响这项附件，原文件和已上传的内容都会保留。
      </p>
      <p>确认后，上传会保持停止，直到你手动继续。</p>
    </div>
    <footer>
      <button ref="cancel" class="secondary" :disabled="working" @click="close">取消</button>
      <button
        class="primary"
        :disabled="working || model.busy.value"
        data-testid="cloud-omit-attachment-confirm"
        @click="confirm"
      >
        确认省略
      </button>
    </footer>
  </dialog>
</template>
