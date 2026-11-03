<script setup lang="ts">
import { computed, onMounted, ref, useId } from 'vue';
import type { TrackView } from '../../services/contracts';
import { toggleBatchSelection } from '../../services/playlistBatch';
import PlaylistBatchTable from '../playlist/PlaylistBatchTable.vue';
import Icon from '../Icon.vue';

const props = defineProps<{
  tracks: TrackView[];
  liked: Set<string>;
  currentId?: string;
  playable: boolean;
}>();
const emit = defineEmits<{
  close: [];
  choose: [kind: 'play' | 'collect' | 'download', tracks: TrackView[]];
}>();
const dialog = ref<HTMLDialogElement>();
const title = useId();
const ids = computed(() => props.tracks.map((track) => track.id));
const selected = ref(new Set<string>());
const chosen = computed(() => props.tracks.filter((track) => selected.value.has(track.id)));
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
function choose(kind: 'play' | 'collect' | 'download') {
  if (chosen.value.length && (kind !== 'play' || props.playable))
    emit('choose', kind, chosen.value);
}
onMounted(() => dialog.value?.showModal());
</script>

<template>
  <dialog
    ref="dialog"
    class="artist-track-batch"
    :aria-labelledby="title"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 :id="title">批量操作歌手歌曲</h2>
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
    <p class="muted">从当前已加载且可用的 {{ tracks.length }} 首歌曲中选择。</p>
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
        播放所选歌曲
      </button>
      <button type="button" class="secondary" :disabled="!chosen.length" @click="choose('collect')">
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
.artist-track-batch[open] {
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
.artist-track-batch::backdrop {
  background: #0006;
}
.artist-track-batch header,
.artist-track-batch footer {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.artist-track-batch header h2,
.artist-track-batch footer span {
  flex: 1;
}
</style>
