<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import type { VoiceSongPurchasedQuality } from '../services/voiceSongs';
import Icon from './Icon.vue';

const props = defineProps<{
  choices: readonly VoiceSongPurchasedQuality[];
  busy: boolean;
  openGuide: () => Promise<void>;
}>();
const emit = defineEmits<{ close: []; confirm: [quality: VoiceSongPurchasedQuality] }>();
const dialog = ref<HTMLDialogElement>();
const selected = ref(props.choices[0]);
const guideBusy = ref(false);
const guideError = ref(false);
let alive = true;
onBeforeUnmount(() => {
  alive = false;
});
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
async function openGuide() {
  if (props.busy || guideBusy.value) return;
  guideBusy.value = true;
  guideError.value = false;
  try {
    await props.openGuide();
  } catch {
    if (alive) guideError.value = true;
  } finally {
    if (alive) guideBusy.value = false;
  }
}
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
      <div class="purchased-quality-heading">
        <h2 id="voice-purchased-quality-title">选择下载音质</h2>
        <button
          type="button"
          class="icon-button purchased-quality-help"
          aria-label="了解音质"
          title="了解音质"
          :disabled="busy || guideBusy"
          @click="openGuide"
        >
          <Icon name="info" aria-hidden="true" />
        </button>
      </div>
    </header>
    <button
      type="button"
      class="icon-button purchased-quality-close"
      aria-label="关闭音质选择"
      :disabled="busy"
      @click="emit('close')"
    >
      <Icon name="close" />
    </button>
    <p v-if="guideError" class="muted purchased-quality-notice" role="status">
      暂时无法打开音质说明，请重试。
    </p>
    <form @submit.prevent="confirm">
      <fieldset class="purchased-quality-list" :disabled="busy" aria-label="下载音质">
        <label v-for="choice in choices" :key="choice" class="purchased-quality-row">
          <input
            v-model="selected"
            name="voice-song-quality"
            type="radio"
            :value="choice"
            :data-voice-purchased-quality="choice"
            :autofocus="choice === choices[0]"
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
  width: min(442px, calc(100vw - 48px));
  border-radius: 12px;
  padding: 0;
}
.purchased-quality-dialog header {
  position: relative;
  justify-content: center;
  min-height: 28px;
  margin: 38px 0 18px;
  padding-inline: 44px;
}
.purchased-quality-heading {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}
.purchased-quality-heading h2 {
  font-weight: 600;
  line-height: 28px;
  overflow-wrap: anywhere;
}
.purchased-quality-help {
  flex: 0 0 26px;
  width: 26px;
  height: 28px;
  justify-content: flex-start;
  margin-inline-end: -10px;
  padding: 0;
}
.purchased-quality-help svg {
  width: 16px;
  height: 16px;
}
.purchased-quality-close {
  position: absolute;
  z-index: 3;
  top: 20px;
  inset-inline-end: 20px;
  padding: 0;
}
.purchased-quality-close svg {
  width: 24px;
  height: 24px;
}
.purchased-quality-notice {
  margin: 0 24px 12px;
  text-align: center;
}
.purchased-quality-dialog form {
  display: flex;
  flex-direction: column;
}
.purchased-quality-list {
  max-height: min(216px, 50dvh);
  overflow-y: auto;
  padding: 0;
}
.purchased-quality-row {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 10px;
  min-height: 42px;
  position: relative;
  margin: 0;
  /* The official 40px inset also leaves a 10px gap for its empty icon slot. */
  padding-inline: clamp(20px, 10vw, 50px) clamp(12px, 5vw, 20px);
  font-size: 16px;
  line-height: 22px;
  cursor: pointer;
}
.purchased-quality-row span {
  min-width: 0;
  overflow-wrap: anywhere;
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
  flex: 0 0 24px;
  width: 24px;
  height: 24px;
  margin-inline-start: auto;
  color: var(--accent);
}
.purchased-quality-dialog footer {
  margin: 20px 0 40px;
  padding: 0 16px;
}
.purchased-quality-dialog .primary {
  width: min(175px, 100%);
  min-width: 0;
  height: 40px;
  padding: 0 8px;
  font-size: 16px;
}
@media (max-height: 580px) {
  .purchased-quality-dialog header {
    margin-top: 28px;
  }
  .purchased-quality-close {
    top: 12px;
  }
  .purchased-quality-dialog footer {
    margin-bottom: 24px;
  }
}
@media (max-width: 400px) {
  .purchased-quality-dialog header {
    padding-inline: 34px;
  }
  .purchased-quality-close {
    inset-inline-end: 8px;
  }
}
</style>
