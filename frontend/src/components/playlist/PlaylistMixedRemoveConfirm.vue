<script setup lang="ts">
import { onMounted, ref } from 'vue';

defineProps<{ name: string; count: number; title?: string }>();
const emit = defineEmits<{ cancel: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-editor-confirm"
    aria-labelledby="mixed-playlist-remove-title"
    @cancel.prevent="emit('cancel')"
  >
    <header><h2 id="mixed-playlist-remove-title">从歌单中删除</h2></header>
    <p v-if="title">将「{{ title }}」从「{{ name }}」中删除？</p>
    <p v-else>将选中的 {{ count }} 首歌曲从「{{ name }}」中删除？</p>
    <p class="muted">本地文件和播放队列将保留。</p>
    <footer>
      <button class="download-button" type="button" autofocus @click="emit('cancel')">取消</button>
      <button class="primary" type="button" @click="emit('confirm')">确认删除</button>
    </footer>
  </dialog>
</template>
