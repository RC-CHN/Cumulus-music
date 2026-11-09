<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { useLocalBatchFavorite } from '../composables/useLocalBatchFavorite';
import type { LocalCollectSort } from '../services/local';
import { modalEntered, modalLeft } from '../services/modal';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{ model: ReturnType<typeof useLocalBatchFavorite>; count: number }>();
const emit = defineEmits<{ close: [] }>();
const { sort, current, busy, target, pending, status } = props.model;
const dialog = ref<HTMLDialogElement>();
const options = [
  { value: 'default', label: '默认排序' },
  { value: 'recently', label: '常用优先' },
] as const;
onMounted(() => {
  if (dialog.value) {
    dialog.value.showModal();
    modalEntered(dialog.value);
  }
});
onBeforeUnmount(() => {
  if (dialog.value) modalLeft(dialog.value);
});
function close() {
  if (!busy.value) emit('close');
}
function sortKey(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key) || target.value) return;
  event.preventDefault();
  const next: LocalCollectSort =
    event.key === 'Home'
      ? 'default'
      : event.key === 'End'
        ? 'recently'
        : sort.value === 'default'
          ? 'recently'
          : 'default';
  props.model.changeSort(next);
  void nextTick(() =>
    dialog.value?.querySelector<HTMLButtonElement>(`[data-collect-sort="${next}"]`)?.focus(),
  );
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-collect-dialog"
    data-local-batch-favorite-dialog
    aria-labelledby="local-batch-favorite-title"
    :aria-busy="busy"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="local-batch-favorite-title">收藏到歌单</h2>
      <button
        class="icon-button collect-close"
        type="button"
        aria-label="关闭歌单操作"
        :disabled="busy"
        autofocus
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p class="collect-selection-summary">将收藏 {{ count }} 首歌曲。</p>
    <div
      v-if="status"
      class="collect-feedback mutation-result"
      role="status"
      data-local-batch-favorite-status
    >
      {{ status }}
      <button
        v-if="pending"
        type="button"
        class="text-button"
        :disabled="busy"
        @click="model.collect(target, true)"
      >
        核对结果
      </button>
    </div>
    <div class="collect-body">
      <div class="collect-sort-tabs" role="tablist" aria-label="收藏歌单排序" @keydown="sortKey">
        <button
          v-for="option in options"
          :key="option.value"
          role="tab"
          type="button"
          :data-collect-sort="option.value"
          :aria-selected="sort === option.value"
          :tabindex="sort === option.value ? 0 : -1"
          :disabled="!!target"
          @click="model.changeSort(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
      <div v-if="current.error || current.loading" class="collect-load" role="status">
        <span>{{ current.error || '正在刷新歌单…' }}</span>
        <button
          v-if="current.error"
          class="text-button"
          type="button"
          :disabled="current.loading || !!target"
          @click="model.load()"
        >
          重试
        </button>
      </div>
      <PlaylistCollectList
        :key="sort"
        :playlists="current.rows"
        :disabled="busy || !!target"
        :include-create="false"
        :selected-id="target"
        :scroll-top="current.top"
        :loading="current.loading || !!current.error || current.next !== null"
        @scroll="current.top = $event"
        @near-end="model.load(true)"
        @select="model.collect($event)"
      />
      <button
        v-if="current.next !== null && current.next !== 0 && !current.error"
        type="button"
        class="text-button collect-more"
        :disabled="current.loading || !!target"
        @click="model.load()"
      >
        {{ current.loading ? '正在加载…' : '加载更多歌单' }}
      </button>
    </div>
    <p v-if="busy" class="collect-status" role="status">正在处理，请稍候…</p>
  </dialog>
</template>
