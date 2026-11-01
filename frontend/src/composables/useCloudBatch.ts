import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import type { CloudClient } from '../services/cloud.ts';
import { readCloudBatch, type CloudBatchData } from '../services/cloudBatch.ts';
import { toggleBatchSelection } from '../services/playlistBatch.ts';
export function useCloudBatch(
  client: CloudClient,
  account: () => AccountView,
  expired: () => void,
) {
  const data = shallowRef<CloudBatchData>(),
    selected = shallowRef(new Set<string>()),
    loading = ref(false),
    error = ref(''),
    progress = ref('');
  const tracks = computed(() => data.value?.tracks ?? []),
    ids = computed(() => tracks.value.map((t) => t.track.id)),
    chosen = computed(() => tracks.value.filter((t) => selected.value.has(t.track.id)));
  const owner = account().epoch;
  let alive = true,
    serial = 0,
    anchor = -1;
  const active = () => alive && account().epoch === owner && account().status === 'authenticated';
  watch(
    () => [account().epoch, account().status],
    () => {
      if (!active()) {
        serial++;
        data.value = undefined;
        selected.value = new Set();
        loading.value = false;
        error.value = '';
        progress.value = '';
      }
    },
  );
  onScopeDispose(() => {
    alive = false;
    serial++;
  });
  async function load() {
    if (!active() || loading.value) return;
    const request = ++serial;
    loading.value = true;
    progress.value = '正在读取完整云盘…';
    try {
      const value = await readCloudBatch(
        client,
        owner,
        () => active() && request === serial,
        (loaded, total) => {
          if (active() && request === serial)
            progress.value = `正在读取云盘歌曲：${loaded} / ${total}`;
        },
      );
      if (!active() || request !== serial) return;
      data.value = value;
      const available = new Set(value.tracks.map((v) => v.track.id));
      selected.value = new Set([...selected.value].filter((id) => available.has(id)));
      anchor = -1;
      error.value = '';
    } catch (e) {
      if (active() && request === serial) {
        const issue = failure(e);
        error.value = issue.message;
        if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
      }
    } finally {
      if (active() && request === serial) loading.value = false;
    }
  }
  function toggle(index: number, range = false) {
    if (loading.value || error.value || !active()) return;
    selected.value = toggleBatchSelection(ids.value, selected.value, index, anchor, range);
    if (!range) anchor = index;
  }
  function all(force = false) {
    if (loading.value || error.value || !active()) return;
    selected.value =
      !force && selected.value.size === ids.value.length ? new Set() : new Set(ids.value);
    anchor = -1;
  }
  function select(index: number) {
    if (loading.value || error.value || !active() || !ids.value[index]) return;
    selected.value = new Set([ids.value[index]!]);
    anchor = index;
  }
  return {
    owner,
    data,
    tracks,
    selected,
    chosen,
    loading,
    error,
    progress,
    active,
    load,
    toggle,
    all,
    select,
  };
}
