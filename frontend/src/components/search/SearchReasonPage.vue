<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import { failure, type TrackView } from '../../services/contracts';
import { useCore } from '../../services/core';
import type { SearchReasonPage, SearchReasonTarget } from '../../services/search';
import type { TrackMenuRequest } from '../../services/trackMenu';
import TrackTable from '../TrackTable.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
import CatalogTrackBatchDialog from '../catalog/CatalogTrackBatchDialog.vue';
import { usePageEntryFocus } from '../../composables/usePageEntryFocus';

const props = defineProps<{
  epoch: string;
  label: string;
  target: SearchReasonTarget;
  liked: Set<string>;
  busyLikes: Set<string>;
  playable: boolean;
}>();
const emit = defineEmits<{
  play: [tracks: TrackView[], index: number];
  download: [tracks: TrackView[]];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  artist: [id: string];
  album: [id: string];
  expired: [];
}>();
const core = useCore();
const result = shallowRef<SearchReasonPage>();
const loading = ref(false);
const error = ref('');
const keyword = ref('');
const retry = ref(0);
const root = ref<HTMLElement>();
const batchButton = ref<HTMLElement>();
const batchOpen = ref(false);
type BatchChoice = {
  result: SearchReasonPage;
  epoch: string;
  kind: 'play' | 'download';
  tracks: TrackView[];
};
let batchChoice: BatchChoice | undefined;
usePageEntryFocus(
  root,
  () => !loading.value,
  () => true,
);
const tracks = computed(() => {
  const term = keyword.value.trim().toLocaleLowerCase();
  return (result.value?.tracks ?? []).filter(
    (track) =>
      !term ||
      [track.name, track.album, ...track.artists].some((label) =>
        label.toLocaleLowerCase().includes(term),
      ),
  );
});
const queueLimit = 500;
const usableTracks = computed(() =>
  (result.value?.tracks ?? []).filter((track) => !track.missing && !track.local),
);
function play(index: number) {
  const start = Math.floor(index / queueLimit) * queueLimit;
  emit('play', tracks.value.slice(start, start + queueLimit), index - start);
}
function chooseBatch(kind: string, tracks: TrackView[]) {
  if (!batchOpen.value || !result.value || (kind !== 'play' && kind !== 'download')) return;
  batchChoice = { result: result.value, epoch: props.epoch, kind, tracks };
  batchOpen.value = false;
}
function finishBatch() {
  const choice = batchChoice;
  batchChoice = undefined;
  if (!choice || choice.epoch !== props.epoch || choice.result !== result.value) return;
  if (choice.kind === 'download') emit('download', choice.tracks);
  else if (props.playable) emit('play', choice.tracks, 0);
}
watch(
  () => [props.epoch, props.target] as const,
  () => {
    batchChoice = undefined;
    batchOpen.value = false;
  },
  { flush: 'sync' },
);
watch(
  () => [props.epoch, props.target, retry.value] as const,
  async ([epoch, target], _, invalidate) => {
    let active = true;
    invalidate(() => {
      active = false;
    });
    loading.value = true;
    error.value = '';
    result.value = undefined;
    keyword.value = '';
    try {
      const reply = await core.searchReasonPage(epoch, target);
      if (active && reply.epoch === epoch) result.value = reply.value;
    } catch (cause) {
      if (!active) return;
      const issue = failure(cause);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
    } finally {
      if (active) loading.value = false;
    }
  },
  { immediate: true },
);
</script>
<template>
  <section ref="root" class="search-reason-page" :aria-busy="loading">
    <header class="page-heading">
      <div>
        <h1>{{ result?.title ?? label }}</h1>
        <p v-if="result?.playCount != null">播放 {{ result.playCount.toLocaleString() }} 次</p>
      </div>
    </header>
    <p v-if="loading" role="status">正在读取推荐歌曲…</p>
    <div v-else-if="error" class="catalog-error" role="alert">
      <p>{{ error }}</p>
      <button type="button" class="secondary" @click="retry++">重试</button>
    </div>
    <template v-else-if="result">
      <div class="search-reason-actions">
        <button
          type="button"
          class="primary"
          :disabled="!usableTracks.length || !playable"
          @click="emit('play', usableTracks.slice(0, queueLimit), 0)"
        >
          <Icon name="play" />
          {{ usableTracks.length > queueLimit ? '播放前 500 首' : '播放全部' }}
        </button>
        <button
          type="button"
          class="secondary"
          :disabled="!usableTracks.length"
          @click="emit('download', usableTracks)"
        >
          <Icon name="download" />
          下载
        </button>
        <button
          ref="batchButton"
          type="button"
          class="secondary"
          :disabled="!usableTracks.length"
          @click="batchOpen = true"
        >
          <Icon name="list" />
          批量操作
        </button>
      </div>
      <div class="search-reason-tools">
        <span>歌曲 · {{ result.tracks.length }}</span>
        <input
          v-model="keyword"
          type="search"
          aria-label="搜索推荐歌曲"
          placeholder="搜索歌曲、歌手或专辑"
        />
      </div>
      <TrackTable
        v-if="tracks.length"
        :tracks="tracks"
        :liked="liked"
        :busy="busyLikes"
        :playable="playable"
        navigable
        @play="play"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @artist="emit('artist', $event)"
        @album="emit('album', $event)"
      />
      <p v-else role="status">{{ keyword ? '没有匹配的歌曲' : '没有可展示的推荐歌曲' }}</p>
    </template>
    <ModalTransition :return-focus="() => batchButton ?? null" @left="finishBatch">
      <CatalogTrackBatchDialog
        v-if="batchOpen"
        :tracks="usableTracks"
        scope="reason"
        :liked="liked"
        :playable="playable"
        :queueable="false"
        :playback-limit="queueLimit"
        @close="batchOpen = false"
        @choose="chooseBatch"
      />
    </ModalTransition>
  </section>
</template>
<style scoped>
.search-reason-page {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
  overflow: auto;
}
.search-reason-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-bottom: 20px;
}
.search-reason-actions > button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: 40px;
  border-radius: 8px;
}
.search-reason-tools {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  padding: 0 0 12px;
}
.search-reason-tools input {
  margin-inline-start: auto;
  min-width: 0;
  padding: 5px 10px;
  border: 1px solid var(--line);
  border-radius: 6px;
}
</style>
