<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { vinylDisplayModes, type VinylDisplayMode } from '../services/vinylDisplay';
import Icon from './Icon.vue';

const props = defineProps<{ mode: VinylDisplayMode }>();
const emit = defineEmits<{ select: [mode: VinylDisplayMode]; close: [] }>();
const dialog = ref<HTMLDialogElement>();

onMounted(() => {
  dialog.value?.showModal();
  dialog.value?.querySelector<HTMLInputElement>('input:checked')?.focus({ preventScroll: true });
});
</script>

<template>
  <dialog
    ref="dialog"
    class="vinyl-mode-sheet"
    aria-labelledby="vinyl-mode-heading"
    @keydown.esc.stop
    @cancel.prevent="emit('close')"
    @click.self="emit('close')"
  >
    <div class="vinyl-mode-sheet-content">
      <header>
        <h2 id="vinyl-mode-heading">播放器模式</h2>
        <button
          type="button"
          class="icon-button"
          aria-label="关闭播放器模式"
          @click="emit('close')"
        >
          <Icon name="close" />
        </button>
      </header>
      <div role="radiogroup" aria-labelledby="vinyl-mode-heading" class="vinyl-mode-choices">
        <label v-for="option in vinylDisplayModes" :key="option.value" class="vinyl-mode-choice">
          <span class="vinyl-mode-preview" :data-preview-mode="option.value" aria-hidden="true">
            <i v-if="option.value !== 'lyric'" />
            <span><b /><b /><b /></span>
          </span>
          <span>
            <input
              type="radio"
              name="vinyl-display-mode"
              :value="option.value"
              :checked="props.mode === option.value"
              @change="emit('select', option.value)"
            />
            {{ option.label }}
          </span>
        </label>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
.vinyl-mode-sheet {
  position: fixed;
  inset: var(--header) 0 var(--player) auto;
  width: min(440px, 100vw);
  height: auto;
  max-height: none;
  max-width: none;
  margin: 0;
  padding: 0;
  color: var(--text);
  background: var(--surface-raised);
  border: 1px solid var(--line);
  border-radius: 12px 0 0 12px;
  box-shadow: -8px 0 32px #0002;
  overflow: auto;
}
.vinyl-mode-sheet::backdrop {
  background: #0004;
}
.vinyl-mode-sheet.modal-enter-from,
.vinyl-mode-sheet.modal-leave-to {
  transform: translateX(28px);
}
.vinyl-mode-sheet-content {
  padding: 22px clamp(12px, 3vw, 24px);
}
.vinyl-mode-sheet-content > header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  margin-bottom: 14px;
}
.vinyl-mode-sheet-content h2 {
  font-size: 18px;
}
.vinyl-mode-choices {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.vinyl-mode-choice {
  min-width: 0;
  display: grid;
  justify-items: center;
  gap: 8px;
  padding: 8px 4px;
  border: 1px solid transparent;
  border-radius: 8px;
  cursor: pointer;
  text-align: center;
}
.vinyl-mode-choice:has(:checked) {
  border-color: var(--accent);
  background: var(--surface-hover);
}
.vinyl-mode-choice input {
  accent-color: var(--accent);
}
.vinyl-mode-preview {
  display: flex;
  align-items: center;
  justify-content: center;
  width: min(78px, 100%);
  height: 42px;
  gap: 4px;
  background: var(--lyrics-background);
  border: 1px solid var(--line);
  border-radius: 5px;
}
.vinyl-mode-preview i {
  width: 20px;
  height: 20px;
  border: 1px solid var(--line);
  border-radius: 50%;
  background: radial-gradient(var(--accent) 0 20%, #393840 24% 43%, #222125 47%);
}
.vinyl-mode-preview > span {
  display: grid;
  gap: 3px;
  width: 23px;
}
.vinyl-mode-preview b {
  height: 2px;
  border-radius: 3px;
  background: var(--muted);
}
.vinyl-mode-preview b:nth-child(2) {
  background: var(--accent);
}
.vinyl-mode-preview[data-preview-mode='cover'] {
  flex-direction: column;
}
.vinyl-mode-preview[data-preview-mode='cover'] b:not(:nth-child(2)) {
  display: none;
}
.vinyl-mode-preview[data-preview-mode='lyric'] > span {
  width: 34px;
}
</style>
