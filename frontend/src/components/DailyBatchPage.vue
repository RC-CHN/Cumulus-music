<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { failure, type TrackPage, type TrackView } from '../services/contracts';
import { toggleBatchSelection } from '../services/playlistBatch';
import type { PlayerModel } from '../composables/usePlayer';
import PlaylistBatchTable from './playlist/PlaylistBatchTable.vue';
import Icon from './Icon.vue';
import '../styles/playlist-batch.css';

const props = defineProps<{
  page?: TrackPage;
  liked: Set<string>;
  currentId?: string;
  loading: boolean;
  error: string;
  player: PlayerModel;
  queueable: boolean;
}>();
const emit = defineEmits<{
  close: [];
  refresh: [];
  expired: [];
  collect: [tracks: TrackView[]];
  download: [tracks: TrackView[]];
}>();
const heading = ref<HTMLElement>();
const selected = ref(new Set<string>());
const tracks = computed(() => props.page?.tracks ?? []);
const ids = computed(() => tracks.value.map((track) => track.id));
const available = computed(() =>
  tracks.value.flatMap((track, index) =>
    selected.value.has(track.id) && !track.missing && !track.local ? [{ track, index }] : [],
  ),
);
const acting = ref(false);
const message = ref('');
const failed = ref(false);
let anchor = -1;
let operation = 0;
watch(
  () => props.page?.revision,
  () => {
    selected.value = new Set();
    anchor = -1;
    operation++;
    acting.value = false;
    message.value = '';
  },
);
onMounted(() => heading.value?.focus({ preventScroll: true }));
onBeforeUnmount(() => operation++);
function toggle(index: number, range: boolean) {
  selected.value = toggleBatchSelection(ids.value, selected.value, index, anchor, range);
  anchor = index;
}
function select(index: number) {
  const id = ids.value[index];
  if (id) selected.value = new Set([id]);
  anchor = index;
}
function all(force?: boolean) {
  selected.value =
    force || selected.value.size !== ids.value.length ? new Set(ids.value) : new Set();
  anchor = -1;
}
async function choose(kind: 'play' | 'queue' | 'collect' | 'download') {
  const revision = props.page?.revision;
  const chosen = available.value;
  if (!revision || !chosen.length || acting.value || props.loading) return;
  if (kind === 'collect')
    return emit(
      'collect',
      chosen.map(({ track }) => track),
    );
  if (kind === 'download')
    return emit(
      'download',
      chosen.map(({ track }) => track),
    );
  if (!props.queueable) return;
  const current = ++operation;
  const indices = chosen.map(({ index }) => index);
  acting.value = true;
  failed.value = false;
  message.value = kind === 'play' ? '正在准备播放…' : '正在添加所选歌曲…';
  try {
    const result =
      kind === 'queue'
        ? await props.player.queueDaily(revision, indices)
        : await props.player.selectForPage({
            kind: 'daily_recommendations_batch',
            revision,
            indices,
            quality: props.player.view.value?.qualityPreferences.quality ?? 'standard',
          });
    if (current !== operation) return;
    if (!result) throw { message: '播放状态已变化，请重新选择' };
    if (kind === 'play' && result.state === 'error') throw { message: '所选歌曲未能播放，请重试' };
    message.value = kind === 'play' ? '正在播放所选歌曲' : '已添加所选至播放列表';
  } catch (error) {
    if (current !== operation) return;
    const issue = failure(error);
    failed.value = true;
    message.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  } finally {
    if (current === operation) acting.value = false;
  }
}
</script>

<template>
  <section
    class="playlist-batch-page daily-batch-page"
    aria-labelledby="daily-batch-title"
    @keydown.esc.prevent="emit('close')"
  >
    <h1 id="daily-batch-title" ref="heading" tabindex="-1">批量操作 · 每日推荐</h1>
    <header class="batch-toolbar">
      <div class="batch-buttons">
        <button
          class="primary batch-play"
          type="button"
          aria-label="播放所选歌曲"
          :disabled="!available.length || !queueable || acting"
          @click="choose('play')"
        >
          <Icon name="play" />
        </button>
        <button
          class="batch-queue"
          type="button"
          :disabled="!available.length || !queueable || acting"
          @click="choose('queue')"
        >
          添加所选至播放列表
        </button>
        <button
          type="button"
          :disabled="!available.length || acting || loading"
          @click="choose('download')"
        >
          下载
        </button>
        <button
          type="button"
          :disabled="!available.length || acting || loading"
          @click="choose('collect')"
        >
          添加到歌单
        </button>
      </div>
      <button class="batch-done" type="button" @click="emit('close')">完成</button>
    </header>
    <div v-if="error" class="error-state batch-error" role="alert">
      <span>{{ error }}</span
      ><button type="button" class="text-button" :disabled="loading" @click="emit('refresh')">
        重新读取
      </button>
    </div>
    <p v-if="loading" role="status">正在读取每日推荐…</p>
    <p v-else-if="!tracks.length && !error" role="status">暂无可批量操作的歌曲。</p>
    <p v-if="page?.daily?.cached" class="muted">当前列表来自本账号上次成功加载的每日推荐。</p>
    <p v-if="tracks.length" class="batch-status muted" role="status">
      已选 {{ selected.size }} 首<template v-if="selected.size > available.length"
        >，其中 {{ selected.size - available.length }} 首暂不可用</template
      >
    </p>
    <div v-if="message" class="batch-operation-feedback" :class="{ failed }" role="status">
      {{ message }}
    </div>
    <PlaylistBatchTable
      v-if="tracks.length"
      :tracks="tracks"
      :selected="selected"
      :liked="liked"
      :current-id="currentId"
      :disabled="acting || loading"
      @toggle="toggle"
      @select="select"
      @all="all"
    />
  </section>
</template>

<style scoped>
h1 {
  margin: 8px 0 16px;
}
</style>
