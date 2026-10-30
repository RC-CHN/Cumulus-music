<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { DownloadModel } from '../../composables/useDownloads';
import type { DownloadSection } from '../../services/downloadPage';
import type { MenuItem } from '../../services/menu';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  model: DownloadModel;
  section: DownloadSection;
  canPlay: boolean;
  locked: boolean;
  preparing: boolean;
}>();
const emit = defineEmits<{ play: []; batch: []; clear: []; settings: [] }>();
const { query, view } = props.model;
const owns = props.model.captureVisit();
const keyword = ref(query.value.keyword),
  field = ref<HTMLInputElement>();
let composing = false,
  timer: ReturnType<typeof setTimeout> | undefined;
const menu = computed<MenuItem[]>(() => [
  {
    key: 'batch',
    label: '批量操作',
    icon: 'list',
    disabled: (!view.value?.tasks.length && !query.value.keyword) || props.locked,
  },
  { key: 'settings', label: '下载设置', icon: 'settings' },
]);
function search() {
  clearTimeout(timer);
  timer = undefined;
  if (!composing && owns()) props.model.filter({ keyword: keyword.value.trim() });
}
function flushSearch() {
  if (timer !== undefined) search();
}
defineExpose({ flushSearch });
function input() {
  clearTimeout(timer);
  if (!composing) timer = setTimeout(search, 100);
}
function compositionStart() {
  composing = true;
  clearTimeout(timer);
}
function clear() {
  keyword.value = '';
  search();
  field.value?.focus({ preventScroll: true });
}
watch([() => query.value.tab, () => query.value.category], () => {
  clearTimeout(timer);
  composing = false;
  keyword.value = query.value.keyword;
});
watch(
  () => query.value.keyword,
  (value) => {
    if (!composing) keyword.value = value;
  },
);
onBeforeUnmount(() => clearTimeout(timer));
</script>
<template>
  <div class="download-toolbar" :class="{ 'download-mv-toolbar': section === 'mv' }">
    <template v-if="section === 'active'">
      <button
        class="download-button"
        :disabled="locked || !view?.activeCount"
        @click="model.command({ kind: 'all', action: 'resume' })"
      >
        <Icon name="download" />全部下载
      </button>
      <button
        class="download-button"
        :disabled="locked || !view?.activeCount"
        @click="model.command({ kind: 'all', action: 'pause' })"
      >
        <Icon name="pause" />全部暂停
      </button>
      <button
        class="download-button"
        :disabled="locked || !view?.activeCount"
        @click="emit('clear')"
      >
        <Icon name="trash" />全部清空
      </button>
    </template>
    <template v-else-if="section !== 'mv'">
      <button class="primary download-play-all" :disabled="!canPlay" @click="emit('play')">
        <span v-if="preparing" class="spinner" /><Icon v-else name="play" />{{
          preparing ? '准备中…' : '播放全部'
        }}
      </button>
      <ActionMenu
        label="更多下载操作"
        :items="menu"
        @select="$event === 'batch' ? emit('batch') : emit('settings')"
      />
      <form class="download-search" :class="{ filled: keyword }" @submit.prevent="search">
        <Icon name="search" /><input
          ref="field"
          v-model="keyword"
          type="search"
          placeholder="搜索"
          :aria-label="section === 'voice' ? '搜索下载的声音' : '搜索下载的歌曲'"
          maxlength="256"
          @input="input"
          @compositionstart="compositionStart"
          @compositionend="
            composing = false;
            search();
          "
        />
        <button
          v-if="keyword"
          type="button"
          class="icon-button"
          aria-label="清空下载搜索"
          @click="clear"
        >
          <Icon name="close" />
        </button>
      </form>
    </template>
  </div>
</template>
