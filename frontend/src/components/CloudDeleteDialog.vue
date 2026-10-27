<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import type { CloudModel } from '../composables/useCloud';
import Icon from './Icon.vue';
const props = defineProps<{ model: CloudModel; ids: string[]; revision: string }>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>();
const { busy, error } = props.model;
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  error.value = '';
});
onUnmounted(() => focus?.focus());
function close() {
  if (!busy.value) emit('close');
}
async function remove() {
  if (await props.model.command({ kind: 'delete', ids: props.ids, revision: props.revision }))
    emit('close');
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog"
    aria-labelledby="cloud-delete-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-delete-title">从云盘删除 {{ ids.length }} 首歌曲？</h2>
      <button class="icon-button" aria-label="关闭删除确认" :disabled="busy" @click="close">
        <Icon name="close" />
      </button>
    </header>
    <p>歌曲将从当前账号的音乐云盘移除。请确认选中的歌曲。</p>
    <p v-if="error" role="alert" class="cloud-error">{{ error }}</p>
    <footer>
      <button class="secondary" :disabled="busy" @click="close">保留歌曲</button
      ><button class="primary" :disabled="busy" @click="remove">
        {{ busy ? '正在确认…' : '删除歌曲' }}
      </button>
    </footer>
  </dialog>
</template>
