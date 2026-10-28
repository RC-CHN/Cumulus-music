<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import {
  equalizerChange,
  equalizerLabels,
  equalizerMessage,
  equalizerPreset,
  equalizerPresets,
} from '../services/equalizer';
import Icon from './Icon.vue';
const props = defineProps<{ player: PlayerModel }>();
const emit = defineEmits<{ close: [] }>();
const { view, busy, error, ready } = props.player;
const current = computed(() => view.value?.equalizer);
const dialog = ref<HTMLDialogElement>();
const gains = ref<number[]>(Array(10).fill(0));
const disabled = computed(
  () => busy.value || !ready.value || !current.value || current.value.damaged,
);
const preset = computed(() => equalizerPreset(gains.value));
let focused: HTMLElement | null = null;
const epoch = view.value?.epoch;
watch(
  () => view.value?.epoch,
  (next) => {
    if (next !== epoch) emit('close');
  },
);
watch(
  () => current.value?.revision,
  () => {
    gains.value = [...(current.value?.settings.gains ?? Array(10).fill(0))];
  },
  { immediate: true },
);
onMounted(() => {
  focused = document.activeElement as HTMLElement;
  dialog.value?.showModal();
});
onUnmounted(() => focused?.focus());
async function commit(enabled: boolean, values = gains.value) {
  if (disabled.value || !current.value) return;
  await props.player.action(equalizerChange(current.value, { enabled, gains: values }));
  if (view.value?.epoch === epoch)
    gains.value = [...(current.value?.settings.gains ?? Array(10).fill(0))];
}
function choose(event: Event) {
  const p = equalizerPresets[Number((event.target as HTMLSelectElement).value)];
  if (p) void commit(true, [...p.gains]);
}
</script>
<template>
  <dialog
    ref="dialog"
    class="equalizer-dialog"
    aria-label="均衡器"
    :data-revision="current?.revision"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2>均衡器</h2>
      <button class="icon-button" aria-label="关闭均衡器" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="equalizer-controls">
      <label
        >预设
        <select aria-label="均衡器预设" :value="preset" :disabled="disabled" @change="choose">
          <option v-if="preset < 0" :value="-1" disabled>自定义</option>
          <option v-for="(p, index) in equalizerPresets" :key="p.name" :value="index">
            {{ p.name }}
          </option>
        </select></label
      >
      <button
        class="equalizer-switch"
        role="switch"
        aria-label="启用均衡器"
        :aria-checked="current?.settings.enabled ?? false"
        :disabled="disabled"
        @click="commit(!current?.settings.enabled)"
      >
        <span></span>{{ current?.settings.enabled ? '开启' : '关闭' }}
      </button>
    </div>
    <div class="equalizer-grid">
      <div class="equalizer-scale" aria-hidden="true">
        <span>+12 dB</span><span>0</span><span>−12 dB</span>
      </div>
      <label v-for="(label, index) in equalizerLabels" :key="label" class="equalizer-band">
        <output>{{ Number(gains[index]!.toFixed(2)) }}</output>
        <span class="equalizer-slider"
          ><input
            type="range"
            min="-12"
            max="12"
            step="0.01"
            aria-orientation="vertical"
            :aria-label="`${label} Hz 增益`"
            :aria-valuetext="`${gains[index]} 分贝`"
            :value="gains[index]"
            :disabled="disabled"
            @input="gains[index] = Number(($event.target as HTMLInputElement).value)"
            @change="commit(true)"
        /></span>
        <span>{{ label }}</span>
      </label>
    </div>
    <div class="equalizer-status" role="status" :data-status="current?.status">
      <span>{{ current ? equalizerMessage(current) : '正在读取设置…' }}</span>
      <button
        v-if="current?.damaged"
        :disabled="busy"
        @click="player.action({ kind: 'repair_equalizer' })"
      >
        修复设置
      </button>
      <button
        v-else-if="current && ['failed', 'unavailable'].includes(current.status)"
        :disabled="busy"
        @click="player.action({ kind: 'retry_equalizer' })"
      >
        重新应用
      </button>
    </div>
    <p v-if="error" class="equalizer-error" role="alert">{{ error }}</p>
    <footer>
      <small>为当前账号保存，下次打开仍会保留。</small
      ><button :disabled="disabled" @click="commit(true, Array(10).fill(0))">恢复平直</button>
    </footer>
  </dialog>
</template>
<style scoped>
.equalizer-dialog {
  width: 628px;
  max-width: calc(100vw - 32px);
  max-height: calc(100dvh - 32px);
  overflow: auto;
  padding: 26px 30px 24px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: 0 18px 70px #0003;
}
.equalizer-dialog::backdrop {
  background: #0004;
}
header,
.equalizer-controls,
footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
header {
  margin-bottom: 24px;
}
h2 {
  font-size: 18px;
  margin: 0;
  font-weight: 600;
}
.equalizer-controls {
  font-size: 14px;
}
select {
  margin-left: 14px;
  min-width: 140px;
  padding: 7px 10px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface);
  color: inherit;
}
.equalizer-switch {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-secondary);
  font-size: 12px;
}
.equalizer-switch span {
  width: 30px;
  height: 18px;
  border-radius: 12px;
  background: var(--line-strong);
  position: relative;
}
.equalizer-switch span::after {
  content: '';
  position: absolute;
  width: 14px;
  height: 14px;
  top: 2px;
  left: 2px;
  border-radius: 50%;
  background: var(--surface);
  transition: left 0.15s;
}
.equalizer-switch[aria-checked='true'] span {
  background: var(--accent-fill);
}
.equalizer-switch[aria-checked='true'] span::after {
  left: 14px;
}
.equalizer-grid {
  display: grid;
  grid-template-columns: 38px repeat(10, minmax(0, 1fr));
  gap: 10px;
  margin: 24px 0 22px;
}
.equalizer-band {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  font-size: 12px;
  min-width: 0;
}
.equalizer-slider {
  --slider-length: 211px;
  display: block;
  position: relative;
  width: 20px;
  height: var(--slider-length);
}
.equalizer-band input {
  /* Rotate a horizontal range: older desktop WebKit renders writing-mode
     ranges with horizontal thumbs even when their box is vertical. */
  appearance: none;
  -webkit-appearance: none;
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%) rotate(-90deg);
  width: var(--slider-length);
  height: 20px;
  background: transparent;
  padding: 0;
  margin: 0;
}
.equalizer-band input::-webkit-slider-runnable-track {
  height: 6px;
  border-radius: 4px;
  background: var(--surface-hover);
}
.equalizer-band input::-webkit-slider-thumb {
  appearance: none;
  -webkit-appearance: none;
  width: 16px;
  height: 16px;
  margin-top: -5px;
  border: 1px solid var(--line-strong);
  border-radius: 50%;
  background: var(--surface);
}
.equalizer-band input::-moz-range-track {
  height: 6px;
  border-radius: 4px;
  background: var(--surface-hover);
}
.equalizer-band input::-moz-range-thumb {
  width: 14px;
  height: 14px;
  border: 1px solid var(--line-strong);
  border-radius: 50%;
  background: var(--surface);
}
.equalizer-band output {
  font-size: 10px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.equalizer-band span {
  color: var(--text-secondary);
}
.equalizer-scale {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  font-size: 10px;
  color: var(--muted);
  padding: 26px 0 27px;
}
.equalizer-status {
  min-height: 32px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}
.equalizer-status button {
  flex-shrink: 0;
  color: var(--accent);
}
.equalizer-error {
  font-size: 12px;
  color: var(--danger);
}
.equalizer-dialog footer {
  border-top: 1px solid var(--line);
  margin-top: 16px;
  padding-top: 18px;
  font-size: 12px;
}
.equalizer-dialog small {
  color: var(--muted);
  font-size: 11px;
}
.equalizer-dialog footer button {
  border: 1px solid var(--line);
  border-radius: 20px;
  padding: 6px 14px;
}
@media (max-width: 800px) {
  .equalizer-dialog {
    padding: 20px 22px;
    width: 590px;
  }
  .equalizer-grid {
    gap: 8px;
  }
  .equalizer-slider {
    --slider-length: 180px;
  }
}
@media (max-height: 650px) {
  .equalizer-grid {
    margin: 16px 0;
  }
  .equalizer-slider {
    --slider-length: 140px;
  }
  header {
    margin-bottom: 16px;
  }
}
</style>
