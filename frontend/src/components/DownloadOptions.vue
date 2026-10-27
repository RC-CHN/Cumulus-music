<script setup lang="ts">
import { qualities } from '../services/player';
import type { DownloadOptions } from '../services/downloads';
const model = defineModel<DownloadOptions>({ required: true });
defineProps<{ voice?: boolean }>();
</script>
<template>
  <div class="download-options">
    <label
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
