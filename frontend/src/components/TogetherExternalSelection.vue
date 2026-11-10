<script setup lang="ts">
import { onMounted, ref } from 'vue';

defineProps<{ busy: boolean; error: string }>();
const emit = defineEmits<{ resolve: [confirm: boolean] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog together-external-selection"
    data-together-external-selection
    aria-labelledby="together-external-title"
    @cancel.prevent="!busy && emit('resolve', false)"
  >
    <p id="together-external-title">该资源暂不支持一起听，正在努力解锁中</p>
    <p v-if="error" class="catalog-error" role="alert">{{ error }}</p>
    <footer>
      <button autofocus :disabled="busy" @click="emit('resolve', false)">取消</button>
      <button class="primary" :disabled="busy" @click="emit('resolve', true)">
        {{ busy ? '正在结束…' : '结束一起听' }}
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.together-external-selection {
  width: 420px;
  max-width: calc(100vw - 32px);
  text-align: center;
}
.together-external-selection footer {
  justify-content: center;
}
</style>
