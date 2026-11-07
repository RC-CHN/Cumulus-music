<script setup lang="ts">
import { onMounted, ref, useId } from 'vue';
defineProps<{ busy: boolean }>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
const heading = useId();
onMounted(() => dialog.value?.showModal());
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog"
    data-message-read-all-confirm
    :aria-labelledby="heading"
    :aria-busy="busy"
    @cancel.prevent="!busy && emit('close')"
    @keydown.esc.stop
  >
    <header><h2 :id="heading">确定清空全部新消息提醒？</h2></header>
    <footer>
      <button class="secondary" type="button" autofocus :disabled="busy" @click="emit('close')">
        取消
      </button>
      <button class="primary" type="button" :disabled="busy" @click="emit('confirm')">
        {{ busy ? '正在清除…' : '确定' }}
      </button>
    </footer>
  </dialog>
</template>
