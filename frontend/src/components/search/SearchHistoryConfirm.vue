<script setup lang="ts">
import { onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
defineProps<{ busy: boolean; error: string }>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="search-history-confirm"
    aria-labelledby="search-history-confirm-title"
    @cancel.prevent="!busy && emit('close')"
  >
    <button
      class="icon-button search-confirm-close"
      aria-label="取消清空搜索历史"
      :disabled="busy"
      @click="emit('close')"
    >
      <Icon name="close" />
    </button>
    <h2 id="search-history-confirm-title">确定清空全部搜索历史吗？</h2>
    <p v-if="error" class="error-state" role="alert">{{ error }}</p>
    <button class="primary" :disabled="busy" @click="emit('confirm')">
      {{ busy ? '正在清空…' : '确认清空搜索历史' }}
    </button>
  </dialog>
</template>
