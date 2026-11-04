<script setup lang="ts">
import { onMounted, ref, useId } from 'vue';
import Icon from '../Icon.vue';
defineProps<{
  title: string;
  description: string;
  busy: boolean;
  error: string;
  confirmLabel?: string;
  hideCancel?: boolean;
}>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
const titleId = useId();
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog settings-confirm"
    :aria-labelledby="titleId"
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 :id="titleId">{{ title }}</h2>
      <button
        class="icon-button"
        aria-label="关闭设置确认"
        :autofocus="hideCancel"
        :disabled="busy"
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <p>{{ description }}</p>
    <p v-if="error" role="alert" class="catalog-error">{{ error }}</p>
    <footer>
      <button
        v-if="!hideCancel"
        class="secondary"
        autofocus
        :disabled="busy"
        @click="emit('close')"
      >
        取消</button
      ><button class="primary" :disabled="busy" @click="emit('confirm')">
        {{ busy ? '正在处理…' : (confirmLabel ?? '确认') }}
      </button>
    </footer>
  </dialog>
</template>
