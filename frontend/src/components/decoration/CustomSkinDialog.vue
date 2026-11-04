<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { useAppearance } from '../../composables/useAppearance';
import { accentSliders, defaultAccent, sliderAccent } from '../../services/appearance';
import Icon from '../Icon.vue';

const emit = defineEmits<{ close: [] }>();
const { customColor, beginCustomPreview } = useAppearance();
const dialog = ref<HTMLDialogElement>();
const preview = beginCustomPreview();
const selected = ref(customColor.value ?? '');
const { hue: initialHue, brightness: initialBrightness } = accentSliders(
  selected.value || defaultAccent,
);
const hue = ref(initialHue);
const brightness = ref(initialBrightness);
const swatches = [
  ['黑色', '#000000'],
  ['玫红', '#f65888'],
  ['粉色', '#f8789b'],
  ['浅紫', '#797df8'],
  ['深蓝', '#5590ea'],
  ['浅蓝', '#53ace4'],
  ['翠绿', '#42b66d'],
  ['浅绿', '#76ce2e'],
  ['金黄', '#d7b017'],
  ['橙色', '#fb8c55'],
  ['珊瑚红', '#f56e6a'],
  ['红色', '#f4514a'],
];
onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => {
  preview.cancel();
  dialog.value?.close();
});
function choose(color: string) {
  ({ hue: hue.value, brightness: brightness.value } = accentSliders(color));
  selected.value = color;
  preview.change(color);
}
function adjust(channel: 'hue' | 'brightness', event: Event) {
  const value = (event.target as HTMLInputElement).valueAsNumber;
  if (!Number.isFinite(value)) return;
  if (channel === 'hue') hue.value = value;
  else brightness.value = value;
  selected.value = sliderAccent(hue.value, brightness.value);
  preview.change(selected.value);
}
function finish(apply: boolean) {
  if (apply && !selected.value) return;
  if (apply) preview.commit(selected.value);
  else preview.cancel();
  dialog.value?.close();
  emit('close');
}
</script>

<template>
  <dialog
    ref="dialog"
    class="skin-color-dialog"
    aria-labelledby="skin-color-title"
    @cancel.prevent="finish(false)"
  >
    <header>
      <h2 id="skin-color-title">按钮颜色自定义</h2>
      <button class="icon-button" type="button" aria-label="关闭颜色设置" @click="finish(false)">
        <Icon name="close" />
      </button>
    </header>
    <div class="skin-color-swatches" aria-label="预设颜色">
      <button
        v-for="[name, color] in swatches"
        :key="color"
        type="button"
        class="skin-swatch"
        :aria-label="name"
        :aria-pressed="selected === color"
        :style="{ '--swatch': color }"
        @click="choose(color)"
      >
        <Icon v-if="selected === color" name="check" />
      </button>
    </div>
    <fieldset class="skin-color-editor">
      <legend>自定义颜色</legend>
      <div
        class="skin-current-color"
        :style="{ '--swatch': selected || defaultAccent }"
        aria-hidden="true"
      ></div>
      <div class="skin-color-sliders">
        <label>
          <span>色相</span>
          <input
            class="skin-hue"
            type="range"
            min="0"
            max="360"
            :value="hue"
            @input="adjust('hue', $event)"
          />
        </label>
        <label>
          <span>亮度</span>
          <input
            type="range"
            min="0"
            max="100"
            :value="brightness"
            @input="adjust('brightness', $event)"
          />
        </label>
      </div>
    </fieldset>
    <footer>
      <button type="button" class="secondary" @click="finish(false)">取消</button>
      <button type="button" class="primary" :disabled="!selected" @click="finish(true)">
        立即使用
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.skin-color-dialog {
  width: min(460px, calc(100vw - 32px));
  max-height: calc(100vh - 32px);
  overflow: auto;
  margin: auto;
  padding: 18px 22px;
  border: 1px solid var(--line);
  border-radius: 12px;
  color: var(--text);
  background: var(--surface-raised);
  box-shadow: var(--shadow);
}
.skin-color-dialog::backdrop {
  background: var(--overlay);
}
.skin-color-dialog header,
.skin-color-dialog footer {
  display: flex;
  align-items: center;
  gap: 10px;
}
.skin-color-dialog header {
  justify-content: space-between;
  margin-bottom: 14px;
}
.skin-color-dialog header h2 {
  margin: 0;
  font-size: 19px;
}
.skin-color-swatches {
  display: grid;
  grid-template-columns: repeat(6, minmax(26px, 1fr));
  gap: 10px;
}
.skin-swatch {
  display: grid;
  place-items: center;
  justify-self: center;
  width: min(100%, 36px);
  aspect-ratio: 1;
  border-radius: 50%;
  color: white;
  background: var(--swatch);
}
.skin-swatch[aria-pressed='true'] .icon {
  filter: drop-shadow(0 1px 2px #000);
}
.skin-swatch:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.skin-color-editor {
  display: grid;
  grid-template-columns: 50px minmax(0, 1fr);
  align-items: center;
  gap: 12px;
  margin: 18px 0;
  padding: 8px 0 0;
  border: 0;
}
.skin-color-editor legend {
  font-size: 14px;
  font-weight: 600;
}
.skin-current-color {
  width: 44px;
  aspect-ratio: 1;
  background: var(--swatch);
  border-radius: 10px;
}
.skin-color-sliders {
  display: grid;
  gap: 4px;
}
.skin-color-sliders label {
  display: grid;
  grid-template-columns: 2.5em minmax(0, 1fr);
  align-items: center;
  font-size: 13px;
  color: var(--text-secondary);
}
.skin-color-sliders input {
  width: 100%;
  accent-color: var(--accent);
}
.skin-color-dialog footer {
  justify-content: end;
}
</style>
