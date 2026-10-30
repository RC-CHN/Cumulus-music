<script setup lang="ts">
import { onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
defineProps<{ count: number; busy: boolean; issue: string }>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog download-batch-confirm"
    aria-labelledby="download-delete-title"
    aria-describedby="download-delete-body"
    :aria-busy="busy"
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 id="download-delete-title">确定删除下载文件？</h2>
      <button class="icon-button" aria-label="关闭删除确认" :disabled="busy" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <p id="download-delete-body">
      将删除选中的 {{ count }} 项下载及其本地歌词。其他歌单中的歌曲不会被移除。
    </p>
    <p v-if="issue" class="form-warning" role="alert">{{ issue }}</p>
    <div class="track-remove-actions">
      <button ref="cancel" :disabled="busy" @click="emit('close')">取消</button
      ><button class="primary" :disabled="busy || !!issue" @click="emit('confirm')">
        {{ busy ? '正在删除…' : '删除' }}
      </button>
    </div>
  </dialog>
</template>
