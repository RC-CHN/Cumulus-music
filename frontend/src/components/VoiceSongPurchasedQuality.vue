<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { VoiceSongPurchasedQuality } from '../services/voiceSongs';
import Icon from './Icon.vue';

const props = defineProps<{
  choices: readonly VoiceSongPurchasedQuality[];
  busy: boolean;
}>();
const emit = defineEmits<{ close: []; confirm: [quality: VoiceSongPurchasedQuality] }>();
const dialog = ref<HTMLDialogElement>();
const selected = ref(props.choices[0]);
const names: Record<VoiceSongPurchasedQuality, string> = {
  vivid: '臻音全景声 (Audio Vivid)',
  sky: '沉浸环绕声 (Surround Audio)',
  jymaster: '超清母带 (Master)',
  jyeffect: '高清臻音 (Spatial Audio)',
  hires: '高解析度无损 (Hi-Res)',
  lossless: '无损 (SQ)',
  exhigh: '极高 (HQ)',
  standard: '标准',
};
onMounted(() => dialog.value?.showModal());
function confirm() {
  const quality = selected.value;
  if (quality && !props.busy && props.choices.includes(quality)) emit('confirm', quality);
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog purchased-quality-dialog"
    data-voice-purchased-download
    aria-labelledby="voice-purchased-quality-title"
    :aria-busy="busy"
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 id="voice-purchased-quality-title">选择下载音质</h2>
      <button
        type="button"
        class="icon-button"
        aria-label="关闭音质选择"
        :disabled="busy"
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <form @submit.prevent="confirm">
      <fieldset class="purchased-quality-list" :disabled="busy" aria-label="下载音质">
        <label v-for="choice in choices" :key="choice" class="purchased-quality-row">
          <input
            v-model="selected"
            name="voice-song-quality"
            type="radio"
            :value="choice"
            :data-voice-purchased-quality="choice"
          />
          <span>{{ names[choice] }}</span>
          <Icon v-if="choice === selected" name="check" aria-hidden="true" />
        </label>
      </fieldset>
      <footer>
        <button type="submit" class="primary" :disabled="busy || !selected">确定</button>
      </footer>
    </form>
  </dialog>
</template>

<style scoped>
.purchased-quality-dialog {
  width: min(416px, calc(100vw - 48px));
}
.purchased-quality-list {
  max-height: min(216px, 50dvh);
  overflow-y: auto;
  padding: 0;
}
.purchased-quality-row {
  display: flex;
  align-items: center;
  min-height: 42px;
  position: relative;
  padding: 0 12px;
  cursor: pointer;
  border-radius: 6px;
}
.purchased-quality-row:hover {
  background: var(--surface-hover, var(--hover));
}
.purchased-quality-row input {
  position: absolute;
  inset: 0;
  margin: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  cursor: inherit;
}
.purchased-quality-row:has(input:focus-visible) {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.purchased-quality-row svg {
  width: 16px;
  height: 16px;
  margin-inline-start: auto;
  color: var(--accent);
}
</style>
