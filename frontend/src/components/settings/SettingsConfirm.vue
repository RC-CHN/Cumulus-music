<script setup lang="ts">
import { onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
defineProps<{ title: string; description: string; busy: boolean; error: string }>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog settings-confirm"
    aria-labelledby="settings-confirm-title"
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 id="settings-confirm-title">{{ title }}</h2>
      <button class="icon-button" aria-label="关闭设置确认" :disabled="busy" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <p>{{ description }}</p>
    <p v-if="error" role="alert" class="catalog-error">{{ error }}</p>
    <footer>
      <button class="secondary" autofocus :disabled="busy" @click="emit('close')">取消</button
      ><button class="primary" :disabled="busy" @click="emit('confirm')">
        {{ busy ? '正在处理…' : '确认' }}
      </button>
    </footer>
  </dialog>
</template>
