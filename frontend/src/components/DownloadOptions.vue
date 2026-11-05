<script setup lang="ts">
import { computed } from 'vue';
import { qualities } from '../services/player';
import type { DownloadOptions, DownloadQualityPreferences } from '../services/downloads';
const model = defineModel<DownloadOptions>({ required: true });
const props = defineProps<{
  voice?: boolean;
  settings?: boolean;
  qualitySettings?: DownloadQualityPreferences;
}>();
const displayed = computed(() =>
  model.value.quality === 'higher' ? 'standard' : model.value.quality,
);
const retained = computed(() => {
  const settings = props.qualitySettings;
  if (!settings || model.value.quality !== settings.options.quality) return undefined;
  if (model.value.quality === 'higher') return '较高（设置中显示为标准）';
  const choice = settings.choices.find((choice) => choice.quality === model.value.quality);
  if (choice?.access !== 'allowed') return label(model.value.quality);
  return undefined;
});
function label(quality: DownloadOptions['quality']) {
  return qualities.find((choice) => choice.value === quality)?.label ?? quality;
}
const captions = {
  allowed: '',
  requires_vip: '需黑胶 VIP',
  requires_svip: '需黑胶 SVIP',
  unknown: '权益待核对',
};
</script>
<template>
  <div class="download-options">
    <template v-if="settings">
      <div
        v-if="qualitySettings"
        class="download-quality-choices"
        role="group"
        aria-label="下载音质"
      >
        <label
          v-for="choice in qualitySettings.choices"
          :key="choice.quality"
          class="private-checkbox"
        >
          <input
            type="radio"
            name="default-download-quality"
            :checked="displayed === choice.quality"
            :disabled="choice.access !== 'allowed'"
            @change="model.quality = choice.quality"
          />
          {{ label(choice.quality) }}
          <small v-if="captions[choice.access]" class="muted">{{ captions[choice.access] }}</small>
        </label>
      </div>
      <p v-if="retained" class="muted">保留已保存的音质：{{ retained }}</p>
    </template>
    <label v-else
      >下载音质<select v-model="model.quality" aria-label="下载音质">
        <option v-for="quality in qualities" :key="quality.value" :value="quality.value">
          {{ quality.label }}
        </option>
      </select></label
    >
    <p class="muted">可下载音质取决于账号权益和内容版权，完成后会显示实际音质。</p>
    <p v-if="voice" class="muted">单集保存为普通音频。</p>
    <label v-if="!voice"
      >保存格式<select v-model="model.container" aria-label="保存格式">
        <option value="audio">普通音频（推荐）</option>
        <option value="ncm">NCM 兼容格式</option>
        <option value="official">跟随官方加密策略</option>
      </select></label
    >
    <p class="muted" v-if="model.container !== 'audio'">加密文件播放时会检查账号权益。</p>
    <div class="download-checkboxes">
      <label class="private-checkbox"
        ><input v-model="model.tags" type="checkbox" />写入歌曲信息</label
      >
      <label class="private-checkbox"
        ><input v-model="model.cover" type="checkbox" />嵌入专辑封面</label
      >
      <label v-if="!voice" class="private-checkbox"
        ><input v-model="model.lyrics" type="checkbox" />同时保存 LRC 歌词</label
      >
    </div>
  </div>
</template>

<style scoped>
.download-quality-choices {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(145px, 100%), 1fr));
  gap: 8px 12px;
}
.download-quality-choices label {
  flex-wrap: wrap;
  align-items: center;
}
</style>
