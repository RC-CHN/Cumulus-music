import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreClient, type TrackPage } from '../services/contracts';
import { readPlaylistBatch, toggleBatchSelection } from '../services/playlistBatch';

export function usePlaylistBatch(
  core: CoreClient,
  account: () => AccountView,
  id: string,
  expired: () => void,
) {
  const data = shallowRef<TrackPage & { revision: string }>(),
    selected = shallowRef(new Set<string>());
  const loading = ref(false),
    error = ref(''),
    progress = ref('');
  const tracks = computed(() => data.value?.tracks ?? []);
  const ids = computed(() => tracks.value.map((t) => t.id));
  const chosen = computed(() => tracks.value.filter((t) => selected.value.has(t.id)));
  const available = computed(() => chosen.value.filter((t) => !t.missing && !t.local));
  let sequence = 0,
    alive = true,
    anchor = -1;
  const owner = account().epoch;
  const active = () => alive && account().epoch === owner && account().status === 'authenticated';
  async function load() {
    if (loading.value) return;
    const current = ++sequence;
    loading.value = true;
    progress.value = '正在读取完整歌单…';
    try {
      const value = await readPlaylistBatch(
        core,
        owner,
        id,
        () => active() && current === sequence,
        (count, total) => {
          progress.value = `正在读取歌单歌曲：${count} / ${total}`;
        },
      );
      if (!active() || current !== sequence) return;
      data.value = value;
      const present = new Set(value.tracks.map((t) => t.id));
      selected.value = new Set([...selected.value].filter((id) => present.has(id)));
      error.value = '';
      anchor = -1;
    } catch (value) {
      if (!active() || current !== sequence) return;
      const issue = failure(value);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
    } finally {
      if (current === sequence) loading.value = false;
    }
  }
  function toggle(index: number, range = false) {
    selected.value = toggleBatchSelection(ids.value, selected.value, index, anchor, range);
    if (!range) anchor = index;
  }
  function all(force = false) {
    selected.value =
      !force && selected.value.size === new Set(ids.value).size ? new Set() : new Set(ids.value);
    anchor = -1;
  }
  function clear() {
    selected.value = new Set();
    anchor = -1;
  }
  function select(index: number) {
    const id = ids.value[index];
    if (!id) return;
    selected.value = new Set([id]);
    anchor = index;
  }
  onMounted(load);
  onBeforeUnmount(() => {
    alive = false;
    sequence++;
  });
  return {
    data,
    tracks,
    chosen,
    available,
    selected,
    loading,
    error,
    progress,
    load,
    toggle,
    select,
    all,
    clear,
  };
}
