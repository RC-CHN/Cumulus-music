<script setup lang="ts">
import { onMounted, ref } from 'vue';
defineProps<{ busy: boolean; error: string }>();
const emit = defineEmits<{ confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog together-room-loss"
    data-together-room-loss
    aria-labelledby="together-room-loss-title"
    @cancel.prevent="!busy && emit('confirm')"
  >
    <p id="together-room-loss-title">一起听已失效，可邀请好友进入新的一起听</p>
    <p v-if="error" class="catalog-error" role="alert">{{ error }}</p>
    <footer>
      <button class="primary" autofocus :disabled="busy" @click="emit('confirm')">
        {{ busy ? '正在结束…' : '结束一起听' }}
      </button>
    </footer>
  </dialog>
</template>
<style scoped>
.together-room-loss {
  width: 420px;
  max-width: calc(100vw - 32px);
  text-align: center;
}
.together-room-loss footer {
  justify-content: center;
}
</style>
