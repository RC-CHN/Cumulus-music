<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import type { LyricAppearance, LyricColors, LyricPreset } from '../../services/desktopLyrics';
import { createLyricSettingsClient } from '../../adapters/desktopLyrics';
import { useDesktopLyricSettings } from '../../composables/useDesktopLyricSettings';
import { useControlFocus } from '../../composables/useControlFocus';
const props = defineProps<{ account: AccountView }>();
const model = useDesktopLyricSettings(createLyricSettingsClient());
const refresh = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
const draft = ref<LyricAppearance>(),
  baseline = ref<LyricAppearance>(),
  baseRevision = ref('');
const copy = <T,>(value: T): T => JSON.parse(JSON.stringify(value));
const dirty = computed(() => JSON.stringify(draft.value) !== JSON.stringify(baseline.value));
const conflict = computed(
  () =>
    !!baseline.value &&
    JSON.stringify(baseline.value) !== JSON.stringify(model.preferences.value?.appearance),
);
function resetDraft() {
  if (!model.preferences.value) return;
  draft.value = copy(model.preferences.value.appearance);
  baseline.value = copy(draft.value);
  baseRevision.value = model.preferences.value.revision;
}
watch(model.preferences, (value) => {
  if (!value) {
    draft.value = undefined;
    baseline.value = undefined;
    baseRevision.value = '';
  } else if (!dirty.value) resetDraft();
  else if (JSON.stringify(value.appearance) === JSON.stringify(baseline.value))
    baseRevision.value = value.revision;
});
watch(
  () => props.account.epoch,
  (epoch) => {
    draft.value = undefined;
    baseline.value = undefined;
    void model.connect(epoch);
  },
  { immediate: true },
);
const palettes: [LyricPreset, string][] = [
  ['red', '网易红'],
  ['gray', '低调灰'],
  ['sunset', '落日晖'],
  ['pink', '樱花粉'],
  ['purple', '香芋紫'],
  ['blue', '天空蓝'],
  ['green', '青草绿'],
  ['golden', '流金岁月'],
];
function choosePalette(value: string) {
  if (!draft.value) return;
  if (value === 'custom') {
    const colors = model.preferences.value?.customPalette ?? model.view.value?.colors;
    if (colors) draft.value.palette = { kind: 'custom', colors: copy(colors) };
  } else draft.value.palette = { kind: 'preset', preset: value as LyricPreset };
}
const custom = computed(() =>
  draft.value?.palette.kind === 'custom' ? draft.value.palette.colors : null,
);
const colorFields: { group: keyof LyricColors; index: number; label: string }[] = [
  { group: 'text', index: 0, label: '未唱上色' },
  { group: 'text', index: 1, label: '未唱下色' },
  { group: 'text', index: 2, label: '已唱上色' },
  { group: 'text', index: 3, label: '已唱下色' },
  { group: 'outline', index: 0, label: '未唱描边' },
  { group: 'outline', index: 1, label: '已唱描边' },
];
const valid = computed(
  () =>
    !!draft.value &&
    Number.isInteger(draft.value.fontSize) &&
    draft.value.fontSize >= 12 &&
    draft.value.fontSize <= 96 &&
    (!custom.value ||
      [...custom.value.text, ...custom.value.outline].every((c) =>
        /^(?:[\da-f]{6}|[\da-f]{8})$/i.test(c),
      )),
);
async function apply() {
  if (!draft.value || !valid.value || !dirty.value) return;
  if (
    await model.send({
      kind: 'change',
      expected_revision: baseRevision.value,
      edit: { kind: 'appearance', value: copy(draft.value) },
    })
  )
    resetDraft();
}
let timer: ReturnType<typeof setInterval> | undefined;
onMounted(() => {
  timer = setInterval(() => void model.status(), 800);
});
onUnmounted(() => {
  clearInterval(timer);
  model.dispose();
});
</script>
<template>
  <section
    class="settings-section desktop-lyric-settings"
    aria-labelledby="settings-desktop-lyrics"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-desktop-lyrics">桌面歌词</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>悬浮显示正在播放的歌词</span
        ><button
          ref="refresh"
          class="text-button"
          aria-label="刷新桌面歌词设置"
          :disabled="model.loading.value || model.busy.value"
          @click="focus.run($event, model.read)"
        >
          刷新
        </button>
      </div>
      <div v-if="model.preferences.value" class="settings-choices">
        <label
          ><input
            type="checkbox"
            aria-label="显示桌面歌词"
            :checked="model.preferences.value.enabled"
            :disabled="!model.canChange.value"
            @click.prevent="
              focus.run($event, () =>
                model.change({ kind: 'enabled', value: !model.preferences.value!.enabled }),
              )
            "
          /><span>显示桌面歌词</span></label
        >
        <button
          v-if="model.preferences.value.enabled && !model.view.value?.visible"
          class="secondary"
          :disabled="model.busy.value"
          @click="focus.run($event, () => model.send('retry'))"
        >
          重试打开
        </button>
        <button
          v-if="model.view.value?.locked"
          class="secondary"
          :disabled="model.busy.value"
          @click="focus.run($event, () => model.send('unlock'))"
        >
          解锁桌面歌词
        </button>
      </div>
      <p class="settings-note" role="status">
        {{
          model.view.value?.opening
            ? '正在打开歌词窗口…'
            : model.view.value?.locked
              ? '歌词已锁定，可在这里或系统托盘解锁。'
              : model.view.value?.visible
                ? '歌词窗口已打开，可拖动和调整大小。'
                : '歌词窗口未打开。'
        }}
      </p>
      <fieldset v-if="draft" class="lyric-style-fields" :disabled="!model.canChange.value">
        <legend class="sr-only">桌面歌词样式</legend>
        <div class="lyric-setting-grid">
          <label
            >辅助歌词<select v-model="draft.auxiliary" aria-label="桌面歌词辅助语言">
              <option value="none">不显示</option>
              <option value="translate">翻译</option>
              <option value="roman">罗马音</option>
            </select></label
          >
          <label
            >行数<select v-model="draft.singleLine" aria-label="桌面歌词行数">
              <option :value="true">单行</option>
              <option :value="false">双行</option>
            </select></label
          >
          <label
            >方向<select v-model="draft.composition" aria-label="桌面歌词方向">
              <option value="horizontal">横排</option>
              <option value="vertical">竖排</option>
            </select></label
          >
          <label
            >对齐<select
              v-if="draft.singleLine"
              v-model="draft.singleAlignment"
              aria-label="桌面歌词单行对齐"
            >
              <option value="center">居中</option>
              <option value="left">居左</option>
              <option value="right">居右</option></select
            ><select v-else v-model="draft.doubleAlignment" aria-label="桌面歌词双行对齐">
              <option value="center">居中</option>
              <option value="split">左右交错</option>
              <option value="left">全部居左</option>
              <option value="right">全部居右</option>
            </select></label
          >
          <label class="font-family"
            >字体<input
              v-model="draft.font"
              aria-label="桌面歌词字体"
              placeholder="系统默认字体"
              maxlength="256"
          /></label>
          <label
            >字号<input
              v-model.number="draft.fontSize"
              type="number"
              min="12"
              max="96"
              step="1"
              aria-label="桌面歌词字号"
          /></label>
          <label
            >配色<select
              aria-label="桌面歌词配色"
              :value="draft.palette.kind === 'preset' ? draft.palette.preset : 'custom'"
              @change="choosePalette(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="[key, label] in palettes" :key="key" :value="key">{{ label }}</option>
              <option value="custom">自定义颜色</option>
            </select></label
          >
        </div>
        <div class="settings-choices">
          <label><input v-model="draft.alwaysOnTop" type="checkbox" />始终置顶</label
          ><label><input v-model="draft.bold" type="checkbox" />粗体</label
          ><label><input v-model="draft.outline" type="checkbox" />文字描边</label>
        </div>
        <div v-if="custom" class="lyric-color-grid">
          <label v-for="field in colorFields" :key="field.label"
            >{{ field.label
            }}<span
              ><input
                type="color"
                :aria-label="field.label + '选择'"
                :value="'#' + custom[field.group][field.index]!.slice(-6)"
                @input="
                  custom[field.group][field.index] = (
                    $event.target as HTMLInputElement
                  ).value.slice(1)
                " /><input
                v-model="custom[field.group][field.index]"
                :aria-label="field.label"
                maxlength="8"
                spellcheck="false" /></span
          ></label>
          <p class="settings-note">颜色可填 6 位 RGB，或 8 位 ARGB（前两位控制透明度）。</p>
        </div>
        <p class="settings-note">
          填写本机字体名称；未安装的字体使用系统回退。锁定状态不会在重启后保留。
        </p>
        <p v-if="!valid" class="catalog-error" role="alert">
          字号须为 12–96 的整数，颜色须为 6 位或 8 位十六进制值。
        </p>
        <p v-if="conflict" class="catalog-error" role="alert">
          样式已在其他位置更改。你的草稿仍保留，请先刷新并检查。
        </p>
        <div class="lyric-setting-actions">
          <button
            class="primary"
            :disabled="!dirty || !valid || conflict"
            @click="focus.run($event, apply)"
          >
            应用样式</button
          ><button class="secondary" :disabled="!dirty && !conflict" @click="resetDraft">
            还原草稿</button
          ><button
            class="text-button"
            @click="focus.run($event, () => model.change({ kind: 'geometry', value: null }))"
          >
            重置窗口位置
          </button>
        </div>
      </fieldset>
      <div v-if="model.view.value?.preferences.damaged" class="settings-recovery" role="alert">
        <p>本机桌面歌词设置损坏，已停止自动打开。</p>
        <button
          class="secondary"
          :disabled="model.busy.value"
          @click="focus.run($event, () => model.send({ kind: 'repair' }))"
        >
          恢复默认桌面歌词设置
        </button>
      </div>
      <p v-if="model.busy.value || model.loading.value" class="settings-note" role="status">
        {{ model.loading.value ? '正在读取…' : '正在应用…' }}
      </p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">已保存。</p>
      <p v-if="model.error.value || model.view.value?.error" class="catalog-error" role="alert">
        {{ model.error.value || model.view.value?.error }}
      </p>
    </div>
  </section>
</template>
<style scoped>
.lyric-style-fields {
  border: 0;
  margin: 14px 0 0;
  padding: 0;
  min-width: 0;
}
.lyric-setting-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 14px;
  margin-bottom: 16px;
}
.lyric-setting-grid label,
.lyric-color-grid label {
  display: flex;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
}
.lyric-setting-grid input,
.lyric-setting-grid select,
.lyric-color-grid input {
  width: 100%;
  min-width: 0;
  border: 1px solid var(--line-strong);
  border-radius: 7px;
  padding: 7px 9px;
  background: var(--surface);
  font: inherit;
}
.font-family {
  grid-column: span 2;
}
.lyric-setting-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
}
.lyric-color-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin-top: 14px;
}
.lyric-color-grid label span {
  display: flex;
  gap: 5px;
}
.lyric-color-grid input[type='color'] {
  width: 36px;
  height: 34px;
  padding: 3px;
  flex-shrink: 0;
}
.lyric-color-grid p {
  grid-column: 1 / -1;
}
@media (max-width: 850px) {
  .lyric-setting-grid,
  .lyric-color-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
