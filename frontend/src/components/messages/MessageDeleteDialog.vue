<script setup lang="ts">
import { onMounted, ref, useId } from 'vue';

defineProps<{ conversation: boolean; busy: boolean }>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
const heading = useId();
onMounted(() => dialog.value?.showModal());
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog"
    data-message-delete-confirm
    :aria-labelledby="heading"
    @cancel.prevent="!busy && emit('close')"
    @keydown.esc.stop
  >
    <header>
      <h2 :id="heading">{{ conversation ? '确定删除私信？' : '确定删除这条私信？' }}</h2>
    </header>
    <p>{{ conversation ? '将删除整个私信会话。' : '将删除选中的这一条私信。' }}</p>
    <footer>
      <button class="secondary" type="button" autofocus :disabled="busy" @click="emit('close')">
        取消
      </button>
      <button class="primary" type="button" :disabled="busy" @click="emit('confirm')">
        {{ busy ? '正在删除…' : '确认删除' }}
      </button>
    </footer>
  </dialog>
</template>
