<script setup lang="ts">
import { computed } from 'vue';
import type { DownloadControl, DownloadTask } from '../../services/downloads';
import { qualities } from '../../services/player';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
const props = defineProps<{
  task: DownloadTask;
  active: boolean;
  locked: boolean;
  repairing: boolean;
  selectionOnly?: boolean;
}>();
const emit = defineEmits<{ control: [action: DownloadControl]; manage: [] }>();
const resume = computed(() => ['paused', 'error'].includes(props.task.status));
const quality = computed(() =>
  props.task.source === 'cloud'
    ? '云盘原文件'
    : props.task.source === 'voice'
      ? '声音'
      : props.task.source === 'mv'
        ? `${props.task.actualResolution ?? props.task.requestedResolution ?? '—'}P`
        : qualities.find((q) => q.value === props.task.actualQuality)?.label,
);
</script>
<template>
  <div class="download-track">
    <img
      v-if="task.track.coverUrl"
      :src="artworkAt(task.track.coverUrl, 120)"
      alt=""
      loading="lazy"
      decoding="async"
      referrerpolicy="no-referrer"
    />
    <span v-else class="track-art"><Icon :name="task.source === 'mv' ? 'video' : 'music'" /></span>
    <div class="download-track-copy">
      <span class="download-track-name ellipsis" :data-tooltip="task.track.name">{{
        task.track.name
      }}</span>
      <small class="download-track-subtitle"
        ><span v-if="quality" class="download-quality">{{ quality }}</span
        ><span v-if="task.encrypted" class="download-quality">NCM</span
        ><span class="ellipsis" :data-tooltip="task.track.artists.join(' / ')">{{
          task.track.artists.join(' / ') || '歌手信息待获取'
        }}</span></small
      >
    </div>
    <div
      v-if="!selectionOnly"
      class="download-row-actions"
      @click.stop
      @dblclick.stop
      @keydown.stop
    >
      <template v-if="active"
        ><button
          class="icon-button"
          :disabled="locked || !!task.fileOperation || task.status === 'cancelling'"
          :aria-label="`${resume ? '继续下载' : '暂停下载'} ${task.track.name}`"
          :data-tooltip="resume ? '开始下载' : '暂停'"
          @click="emit('control', resume ? 'resume' : 'pause')"
        >
          <Icon :name="resume ? 'download' : 'pause'" />
        </button>
        <button
          class="icon-button"
          :disabled="locked || !!task.fileOperation || task.status === 'cancelling'"
          :aria-label="`取消下载 ${task.track.name}`"
          data-tooltip="删除下载"
          @click="emit('control', 'cancel')"
        >
          <Icon name="trash" /></button
      ></template>
      <template v-else
        ><button
          v-if="task.cleanupPending"
          class="text-button download-repair"
          :disabled="locked || repairing"
          @click="emit('control', 'repair')"
        >
          修复
        </button>
        <button
          class="icon-button"
          :aria-label="`管理下载 ${task.track.name}`"
          data-tooltip="管理下载文件"
          @click="emit('manage')"
        >
          <Icon name="more" /></button
      ></template>
    </div>
  </div>
</template>
