<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { CloseBehavior } from '../services/window';
import Icon from './Icon.vue';
defineProps<{ busy: boolean; available: boolean; damaged: boolean; error: string }>();
const behavior = defineModel<CloseBehavior>('behavior', { required: true });
const remember = defineModel<boolean>('remember', { required: true });
const emit = defineEmits<{ cancel: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog window-close-dialog"
    aria-labelledby="close-window-title"
    @cancel.prevent="!busy && emit('cancel')"
  >
    <header>
      <h2 id="close-window-title">关闭提示</h2>
      <button class="icon-button" aria-label="取消关闭" :disabled="busy" @click="emit('cancel')">
        <Icon name="close" />
      </button>
    </header>
    <p>关闭窗口后，你希望：</p>
    <fieldset class="close-choices" :disabled="busy">
      <legend class="sr-only">关闭方式</legend>
      <label
        ><input
          v-model="behavior"
          type="radio"
          name="close-behavior"
          value="tray"
          :disabled="!available"
        /><span>留在系统托盘，继续播放音乐</span></label
      >
      <label
        ><input v-model="behavior" type="radio" name="close-behavior" value="exit" /><span
          >退出 Cumulus</span
        ></label
      >
    </fieldset>
    <p v-if="!available" class="muted" role="status">
      系统托盘暂不可用。可以保留窗口，或选择退出。
    </p>
    <p v-if="damaged" class="muted">关闭设置需要恢复，本次不会保存选择。可在设置中修复。</p>
    <label class="close-remember"
      ><input
        v-model="remember"
        type="checkbox"
        name="close-remember"
        :disabled="busy || damaged"
      />不再提醒（可在设置中修改）</label
    >
    <p v-if="error" class="catalog-error" role="alert">{{ error }}</p>
    <footer>
      <button class="secondary" autofocus :disabled="busy" @click="emit('cancel')">保留窗口</button
      ><button
        class="primary"
        :disabled="busy || (behavior === 'tray' && !available)"
        @click="emit('confirm')"
      >
        {{ busy ? '正在处理…' : '确定' }}
      </button>
    </footer>
  </dialog>
</template>
<style scoped>
.window-close-dialog {
  width: 460px;
  max-width: calc(100vw - 32px);
}
.close-choices {
  border: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 18px;
  margin: 22px 0;
  min-width: 0;
}
.close-choices label,
.close-remember {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 10px;
  line-height: 1.6;
  margin: 0;
}
.window-close-dialog input[type='radio'],
.window-close-dialog input[type='checkbox'] {
  width: 16px;
  height: 16px;
  padding: 0;
  margin: 0;
  accent-color: var(--accent);
  flex-shrink: 0;
}
.close-remember {
  font-size: 13px;
  margin-top: 24px;
}
.close-choices label:has(input:disabled) {
  color: var(--muted);
}
</style>
