<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { useControlFocus } from '../composables/useControlFocus';
import {
  equalizerCanRetry,
  equalizerGainLabel,
  equalizerLabels,
  equalizerMessage,
  equalizerPresetOption,
  equalizerPresets,
  equalizerTone,
  equalizerToneEnabled,
} from '../services/equalizer';
import type { EqualizerPreset } from '../services/player';
import { failure } from '../services/contracts';
import type { AccountView } from '../services/contracts';
import EffectDirectory from './EffectDirectory.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  player: PlayerModel;
  account: AccountView;
  initialEffectManagement?: boolean;
}>();
const emit = defineEmits<{ close: []; login: [] }>();
const { view, ready, equalizerDraft } = props.player;
const { settings, preset, draft, pending, discarding, blocked, error, stage, applyDraft, discard } =
  equalizerDraft;
const current = computed(() => view.value?.equalizer);
const parametric = computed(() => view.value?.boundParametric);
const parametricStatus = {
  configured: '播放器已配置独立参数均衡。',
  bypassed_for_source: '当前音源已旁路独立参数均衡。',
  deferred: '独立参数均衡等待音频加载。',
  detach_required: '独立参数均衡需要清理。',
  processor_failed: '独立参数均衡处理失败，已在本机停用。',
};
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const presetControl = ref<HTMLSelectElement>();
const actionFocus = useControlFocus(
  () => view.value?.epoch,
  () => (presetControl.value?.disabled === false ? presetControl.value : closeButton.value),
);
const directoryOpen = ref(false);
const directoryOpener = ref<HTMLButtonElement>();
const managementOnly = ref(false);
const headphoneManagement = ref(false);
const gains = computed(() => settings.value?.gains ?? Array<number>(10).fill(0));
const tone = computed(() => equalizerTone(settings.value));
const storedToneBypassed = computed(
  () =>
    !equalizerToneEnabled(settings.value) &&
    (tone.value.bass_db !== 0 || tone.value.treble_db !== 0),
);
const toneBands = [
  { key: 'bass_db', label: '低音' },
  { key: 'treble_db', label: '高音' },
] as const;
const commandPending = ref(false);
const commandError = ref('');
const savedMessage = ref('');
const disabled = computed(
  () =>
    !ready.value ||
    !current.value ||
    current.value.damaged ||
    commandPending.value ||
    discarding.value,
);
const saved = computed(() => current.value?.saved_custom);
const displayedPreset = computed(() =>
  equalizerPresetOption(preset.value, gains.value, saved.value),
);
const canSave = computed(
  () => !disabled.value && !pending.value && !draft.value && displayedPreset.value === -1,
);
let focused: HTMLElement | null = null;
const epoch = view.value?.epoch;
watch(
  () => view.value?.epoch,
  (next) => {
    if (next !== epoch) emit('close');
  },
);
onMounted(() => {
  focused = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  if (props.initialEffectManagement) {
    directoryOpener.value?.focus({ preventScroll: true });
    openDirectory(true);
  }
});
onUnmounted(() => focused?.focus());
function openDirectory(localOnly = false, headphones = false) {
  managementOnly.value = localOnly;
  headphoneManagement.value = headphones;
  directoryOpen.value = true;
}
function change(
  enabled: boolean,
  values = gains.value,
  enhancement = tone.value,
  selected?: EqualizerPreset,
  toneEnabled = equalizerToneEnabled(settings.value),
  keepBypass = false,
) {
  if (disabled.value) return;
  commandError.value = '';
  savedMessage.value = '';
  stage(
    {
      enabled,
      gains: values,
      ...enhancement,
      tone_enabled: toneEnabled,
      master_bypassed: keepBypass,
    },
    selected,
  );
}
function toggleMaster() {
  if (disabled.value || !settings.value) return;
  commandError.value = '';
  savedMessage.value = '';
  stage({ ...settings.value, master_bypassed: !settings.value.master_bypassed });
}
function toggleGeq() {
  change(
    !settings.value?.enabled,
    gains.value,
    tone.value,
    undefined,
    equalizerToneEnabled(settings.value),
    settings.value?.master_bypassed,
  );
}
function choose(event: Event) {
  const selected = Number((event.target as HTMLSelectElement).value);
  if (selected === -2 && saved.value) {
    change(true, [...saved.value], tone.value, { kind: 'saved' });
    return;
  }
  const p = equalizerPresets[selected];
  if (p) change(true, [...p.gains], tone.value, { kind: 'built_in', index: selected });
}
function changeBand(index: number, value: number) {
  const values = [...gains.value];
  values[index] = value;
  change(true, values, tone.value, { kind: 'manual' });
}
function slide(index: number, event: Event) {
  changeBand(index, Number((event.target as HTMLInputElement).value));
}
function nudgeBand(index: number, event: KeyboardEvent) {
  if (disabled.value || event.altKey || event.ctrlKey || event.metaKey) return;
  const delta =
    event.key === 'ArrowUp' || event.key === 'ArrowRight'
      ? 1
      : event.key === 'ArrowDown' || event.key === 'ArrowLeft'
        ? -1
        : 0;
  if (!delta) return;
  event.preventDefault();
  const currentGain = Number(gains.value[index]!.toFixed(2));
  const gain = Math.max(-12, Math.min(12, Number((currentGain + delta).toFixed(2))));
  if (gain !== currentGain) changeBand(index, gain);
}
function slideTone(key: (typeof toneBands)[number]['key'], event: Event) {
  const value = Number((event.target as HTMLInputElement).value);
  if (!Number.isInteger(value) || Math.abs(value) > 10) return;
  const next = { ...tone.value, [key]: value };
  change(
    settings.value?.enabled ?? false,
    gains.value,
    next,
    undefined,
    next.bass_db !== 0 || next.treble_db !== 0,
  );
}
function resetTone() {
  change(
    settings.value?.enabled ?? false,
    gains.value,
    { bass_db: 0, treble_db: 0 },
    undefined,
    false,
  );
}
async function saveCustom() {
  const expected = current.value;
  if (!canSave.value || !expected) return;
  commandPending.value = true;
  commandError.value = '';
  savedMessage.value = '';
  try {
    const next = await props.player.configureEqualizer({
      kind: 'save_custom_equalizer',
      expected_revision: expected.revision,
    });
    if (next.epoch === epoch) savedMessage.value = '已保存自定义预设';
  } catch (issue) {
    if (view.value?.epoch === epoch) commandError.value = failure(issue).message;
  } finally {
    commandPending.value = false;
  }
}
async function maintain(kind: 'retry_equalizer' | 'repair_equalizer') {
  if (!ready.value || pending.value || commandPending.value || draft.value) return;
  commandPending.value = true;
  commandError.value = '';
  try {
    await props.player.configureEqualizer({ kind });
  } catch (issue) {
    if (view.value?.epoch === epoch) commandError.value = failure(issue).message;
  } finally {
    commandPending.value = false;
  }
}
</script>
<template>
  <dialog
    ref="dialog"
    class="equalizer-dialog"
    aria-label="均衡器"
    :data-revision="current?.revision"
    :data-draft-state="blocked ? 'blocked' : draft ? 'queued' : 'settled'"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2>均衡器</h2>
      <button ref="closeButton" class="icon-button" aria-label="关闭均衡器" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="equalizer-body">
      <div class="equalizer-master">
        <button
          class="equalizer-switch"
          type="button"
          role="switch"
          aria-label="启用均衡器和低高音增强"
          :aria-checked="!!settings && !settings.master_bypassed"
          :disabled="disabled"
          @click="toggleMaster"
        >
          <span></span>{{ settings?.master_bypassed ? '均衡器与低高音已暂停' : '均衡器与低高音' }}
        </button>
        <small v-if="settings?.master_bypassed">调整预设或增益会重新开启音效。</small>
        <button ref="directoryOpener" type="button" @click="openDirectory()">音效目录</button>
        <button type="button" @click="openDirectory(false, true)">耳机音效</button>
      </div>
      <div v-if="parametric" class="equalizer-independent" :data-parametric-state="parametric">
        <p role="status">
          {{
            parametric === 'configured' && view?.parametricProcessing
              ? '独立参数均衡正在处理音频。'
              : parametricStatus[parametric]
          }}不受上方开关控制。
        </p>
        <button type="button" @click="openDirectory(true)">管理独立参数均衡</button>
      </div>
      <div class="equalizer-controls">
        <label
          >预设
          <select
            ref="presetControl"
            aria-label="均衡器预设"
            :value="displayedPreset"
            :disabled="disabled"
            @change="choose"
          >
            <option v-if="displayedPreset === -1" :value="-1" disabled>未保存的自定义</option>
            <option v-if="saved" :value="-2">已保存的自定义</option>
            <option v-for="(p, index) in equalizerPresets" :key="p.name" :value="index">
              {{ p.name }}
            </option>
          </select></label
        >
        <button
          class="equalizer-switch"
          role="switch"
          aria-label="启用均衡器"
          :aria-checked="settings?.enabled ?? false"
          :disabled="disabled"
          @click="toggleGeq"
        >
          <span></span>{{ settings?.enabled ? '开启' : '关闭' }}
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
              :aria-valuetext="`${equalizerGainLabel(gains[index]!)} 分贝`"
              :value="gains[index]"
              :disabled="disabled"
              @input="slide(index, $event)"
              @change="slide(index, $event)"
              @keydown="nudgeBand(index, $event)"
          /></span>
          <span class="equalizer-band-frequency">{{ label }}</span>
          <span class="equalizer-band-value" aria-hidden="true">
            {{ equalizerGainLabel(gains[index]!) }} dB
          </span>
        </label>
      </div>
      <fieldset class="equalizer-tone" :disabled="disabled">
        <legend>增强音效</legend>
        <small v-if="storedToneBypassed" class="equalizer-tone-note">低音和高音增强已旁路</small>
        <label v-for="{ key, label } in toneBands" :key="key">
          <span>{{ label }}</span>
          <input
            type="range"
            min="-10"
            max="10"
            step="1"
            :aria-label="label + '增益'"
            :aria-valuetext="tone[key] + ' 分贝'"
            :value="tone[key]"
            @input="slideTone(key, $event)"
            @change="slideTone(key, $event)"
          />
          <output>{{ tone[key] }} dB</output>
        </label>
        <button
          type="button"
          :disabled="!tone.bass_db && !tone.treble_db"
          @click="actionFocus.run($event, resetTone)"
        >
          重置低音和高音
        </button>
      </fieldset>
      <div class="equalizer-status" role="status" :data-status="current?.status">
        <span>{{
          discarding
            ? '正在读取播放器当前设置…'
            : blocked
              ? '草稿尚未确认保存'
              : pending || draft
                ? '正在应用当前调整…'
                : current
                  ? equalizerMessage(current)
                  : '正在读取设置…'
        }}</span>
        <button
          v-if="current?.damaged"
          :disabled="!ready || pending || commandPending || !!draft"
          @click="actionFocus.run($event, () => maintain('repair_equalizer'))"
        >
          修复设置
        </button>
        <button
          v-else-if="current && equalizerCanRetry(current)"
          :disabled="!ready || pending || commandPending || !!draft"
          @click="actionFocus.run($event, () => maintain('retry_equalizer'))"
        >
          重新应用
        </button>
      </div>
      <p v-if="error || commandError" class="equalizer-error" role="alert">
        {{ error || commandError }}
      </p>
      <div v-if="blocked" class="equalizer-status">
        <button :disabled="!ready || pending" @click="actionFocus.run($event, applyDraft)">
          读取当前设置并应用草稿
        </button>
        <button :disabled="!ready || pending" @click="actionFocus.run($event, discard)">
          舍弃草稿
        </button>
      </div>
    </div>
    <footer>
      <small aria-live="polite">{{ savedMessage || '为当前账号保存，下次打开仍会保留。' }}</small>
      <button :disabled="!canSave" @click="actionFocus.run($event, saveCustom)">
        保存自定义预设
      </button>
      <button
        :disabled="disabled"
        @click="change(true, [...equalizerPresets[0].gains], tone, { kind: 'built_in', index: 0 })"
      >
        恢复平直
      </button>
    </footer>
  </dialog>
  <EffectDirectory
    v-if="directoryOpen"
    :account="account"
    :binding-status="parametric"
    :parametric-processing="view?.parametricProcessing"
    :device-parametric="view?.deviceParametric"
    :initial-management-only="managementOnly"
    :headphone-management="headphoneManagement"
    @close="directoryOpen = false"
    @equalizer="directoryOpen = false"
    @login="emit('login')"
  />
</template>
<style scoped>
.equalizer-dialog {
  width: 628px;
  max-width: calc(100vw - 32px);
  max-height: calc(100dvh - 32px);
  overflow: hidden;
  padding: 26px 30px 24px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: 0 18px 70px #0003;
}
/* Keep close and reset available while the controls scroll on short windows. */
.equalizer-dialog[open] {
  display: flex;
  flex-direction: column;
}
.equalizer-dialog > header,
.equalizer-dialog > footer {
  flex: none;
}
.equalizer-body {
  min-height: 0;
  overflow-y: auto;
}
.equalizer-master {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px 12px;
  margin-bottom: 10px;
}
.equalizer-master small {
  color: var(--text-secondary);
}
.equalizer-independent {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin-bottom: 10px;
  font-size: 12px;
  color: var(--text-secondary);
}
.equalizer-independent p {
  margin: 0;
}
.equalizer-independent button {
  color: var(--accent);
}
.equalizer-dialog::backdrop {
  background: #0004;
}
.equalizer-tone {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr)) auto;
  align-items: center;
  gap: 8px 14px;
  margin: 0 0 14px;
  padding: 8px 0 0;
  border: 0;
  min-width: 0;
}
.equalizer-tone-note {
  grid-column: 1 / -1;
  color: var(--text-secondary);
}
.equalizer-tone label {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 6px;
}
.equalizer-tone input {
  width: 100%;
  min-width: 0;
  accent-color: var(--accent-fill);
}
.equalizer-tone output {
  min-width: 4ch;
}
@media (max-width: 640px) {
  .equalizer-tone {
    grid-template-columns: minmax(0, 1fr);
  }
  .equalizer-dialog footer {
    flex-wrap: wrap;
    gap: 8px;
  }
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
  appearance: none;
  -webkit-appearance: none;
  margin-left: 14px;
  min-width: 140px;
  padding: 9px 32px 9px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface);
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='8' viewBox='0 0 12 8'%3E%3Cpath d='m1 1 5 5 5-5' fill='none' stroke='%2392919a' stroke-width='1.5'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 12px center;
  color: inherit;
  font: inherit;
  font-size: 13px;
}
.equalizer-switch {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  border-radius: 6px;
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
  transition: transform 0.15s;
}
.equalizer-switch[aria-checked='true'] span {
  background: var(--accent-fill);
}
.equalizer-switch[aria-checked='true'] span::after {
  transform: translateX(12px);
}
.equalizer-grid {
  display: grid;
  grid-template-columns: 38px repeat(10, minmax(0, 1fr));
  gap: 10px;
  margin: 24px 0 22px;
}
.equalizer-band {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  font-size: 12px;
  min-width: 0;
}
.equalizer-band-value {
  display: none;
  position: absolute;
  z-index: 1;
  inset-block-start: -19px;
  inset-inline-start: 50%;
  transform: translateX(-50%);
  border: 1px solid var(--line);
  border-radius: 5px;
  padding: 1px 5px;
  background: var(--surface-raised);
  color: var(--text);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  pointer-events: none;
}
.equalizer-band:focus-within .equalizer-band-value,
.equalizer-band:not(:has(input:disabled)):hover .equalizer-band-value {
  display: block;
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
  border: 2px solid var(--surface-raised);
  border-radius: 50%;
  background: var(--text-secondary);
  box-shadow: 0 0 0 1px var(--muted);
}
.equalizer-band input::-moz-range-track {
  height: 6px;
  border-radius: 4px;
  background: var(--surface-hover);
}
.equalizer-band input::-moz-range-thumb {
  width: 12px;
  height: 12px;
  border: 2px solid var(--surface-raised);
  border-radius: 50%;
  background: var(--text-secondary);
  box-shadow: 0 0 0 1px var(--muted);
}
.equalizer-band output {
  font-size: 11px;
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
