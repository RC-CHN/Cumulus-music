<script setup lang="ts">
import { computed, onMounted, ref, useId } from 'vue';
import type { TrackView } from '../../services/contracts';
import { toggleBatchSelection } from '../../services/playlistBatch';
import PlaylistBatchTable from '../playlist/PlaylistBatchTable.vue';
import Icon from '../Icon.vue';

const props = defineProps<{
  tracks: TrackView[];
  scope: 'artist' | 'album' | 'reason';
  liked: Set<string>;
  currentId?: string;
  playable: boolean;
  queueable: boolean;
  playbackLimit?: number;
}>();
const emit = defineEmits<{
  close: [];
  choose: [kind: 'play' | 'collect' | 'download' | 'queue', tracks: TrackView[]];
}>();
const dialog = ref<HTMLDialogElement>();
const title = useId();
const ids = computed(() => props.tracks.map((track) => track.id));
const selected = ref(new Set<string>());
const chosen = computed(() => props.tracks.filter((track) => selected.value.has(track.id)));
const scopeLabel = computed(() => ({ artist: '歌手', album: '专辑', reason: '推荐' })[props.scope]);
let anchor = -1;
function toggle(index: number, range: boolean) {
  selected.value = toggleBatchSelection(ids.value, selected.value, index, anchor, range);
  anchor = index;
}
function select(index: number) {
  const id = ids.value[index];
  if (!id) return;
  selected.value = new Set([id]);
  anchor = index;
}
function all(force?: boolean) {
  selected.value =
    force || selected.value.size !== ids.value.length ? new Set(ids.value) : new Set();
  anchor = -1;
}
function choose(kind: 'play' | 'collect' | 'download' | 'queue') {
  if (!chosen.value.length || (kind === 'play' && !props.playable)) return;
  if (kind === 'queue' && (props.scope !== 'album' || !props.queueable)) return;
  if (kind === 'collect' && props.scope === 'reason') return;
  emit(
    'choose',
    kind,
    kind === 'play'
      ? chosen.value.slice(0, props.playbackLimit ?? chosen.value.length)
      : chosen.value,
  );
}
onMounted(() => dialog.value?.showModal());
</script>

<template>
  <dialog
    ref="dialog"
    class="catalog-track-batch"
    :class="`${scope}-track-batch`"
    :aria-labelledby="title"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 :id="title">批量操作{{ scopeLabel }}歌曲</h2>
      <button
        type="button"
        class="icon-button"
        aria-label="关闭批量操作"
        autofocus
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <p class="muted">
      从{{ scope === 'album' ? '此专辑可用的' : '当前已加载且可用的' }}
      {{ tracks.length }} 首歌曲中选择。
    </p>
    <PlaylistBatchTable
      :tracks="tracks"
      :selected="selected"
      :liked="liked"
      :current-id="currentId"
      :disabled="false"
      @toggle="toggle"
      @select="select"
      @all="all"
    />
    <footer>
      <span role="status">已选 {{ chosen.length }} 首</span>
      <button
        type="button"
        class="primary"
        :disabled="!chosen.length || !playable"
        @click="choose('play')"
      >
        {{
          playbackLimit && chosen.length > playbackLimit
            ? `播放所选的前 ${playbackLimit} 首`
            : '播放所选歌曲'
        }}
      </button>
      <button
        v-if="scope === 'album'"
        type="button"
        class="secondary"
        :disabled="!chosen.length || !queueable"
        @click="choose('queue')"
      >
        添加所选至播放列表
      </button>
      <button
        v-if="scope !== 'reason'"
        type="button"
        class="secondary"
        :disabled="!chosen.length"
        @click="choose('collect')"
      >
        添加到歌单
      </button>
      <button
        type="button"
        class="secondary"
        :disabled="!chosen.length"
        @click="choose('download')"
      >
        下载所选歌曲
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.catalog-track-batch[open] {
  display: flex;
  flex-direction: column;
  width: min(730px, calc(100vw - 30px));
  height: min(580px, calc(100dvh - 30px));
  padding: 18px;
  gap: 8px;
  color: var(--text);
  background: var(--surface-raised);
  border: 1px solid var(--line);
  border-radius: 14px;
}
.catalog-track-batch::backdrop {
  background: #0006;
}
.catalog-track-batch header,
.catalog-track-batch footer {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.catalog-track-batch header h2,
.catalog-track-batch footer span {
  flex: 1;
}
</style>
