<script setup lang="ts">
import { nextTick, ref } from 'vue';
import { downloadSections, type DownloadSection } from '../../services/downloadPage';
import Icon from '../Icon.vue';
defineProps<{ section: DownloadSection; directory: boolean }>();
const emit = defineEmits<{ select: [section: DownloadSection]; open: []; settings: [] }>();
const tabs = ref<HTMLElement>();
async function navigate(event: KeyboardEvent, index: number) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? 3
        : (index + (event.key === 'ArrowRight' ? 1 : -1) + 4) % 4;
  emit('select', downloadSections[next]!.id);
  await nextTick();
  tabs.value
    ?.querySelectorAll<HTMLButtonElement>('[role="tab"]')
    [next]?.focus({ preventScroll: true });
}
</script>
<template>
  <header class="download-navigation">
    <h1 class="sr-only">下载管理</h1>
    <div ref="tabs" class="download-tabs" role="tablist" aria-label="下载分类">
      <button
        v-for="(tab, index) in downloadSections"
        :key="tab.id"
        :id="`download-tab-${tab.id}`"
        role="tab"
        :aria-selected="section === tab.id"
        aria-controls="download-content"
        :tabindex="section === tab.id ? 0 : -1"
        :class="{ selected: section === tab.id }"
        @click="emit('select', tab.id)"
        @keydown="navigate($event, index)"
      >
        {{ tab.label }}
      </button>
    </div>
    <div class="download-navigation-actions">
      <button class="text-button" :disabled="!directory" @click="emit('open')">打开目录</button>
      <button
        class="icon-button"
        aria-label="下载设置"
        data-tooltip="下载设置"
        @click="emit('settings')"
      >
        <Icon name="settings" />
      </button>
    </div>
  </header>
</template>
