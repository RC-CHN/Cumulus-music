import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import type { DownloadClient, DownloadQuery, DownloadTask } from '../services/downloads';
import {
  batchDownloadQuery,
  nextBatchOrder,
  orderedDownloads,
  type BatchCategory,
  type BatchOrder,
} from '../services/downloadBatch';
import { toggleBatchSelection } from '../services/playlistBatch';
import type { DownloadModel } from './useDownloads';

export function useDownloadBatch(
  core: DownloadClient,
  account: () => AccountView,
  query: DownloadQuery,
  category: BatchCategory,
  downloads: DownloadModel,
  expired: () => void,
) {
  const owner = account().epoch,
    queryAll = batchDownloadQuery(query, category);
  const records = shallowRef<DownloadTask[]>([]),
    selected = shallowRef(new Set<string>()),
    order = ref<BatchOrder>();
  const loading = ref(true),
    error = ref(''),
    migration = ref(false);
  const rows = computed(() => orderedDownloads(records.value, order.value)),
    ids = computed(() => rows.value.map((t) => t.id));
  // The official action filters the original records by offlineId, independently of visual sort.
  const chosen = computed(() => records.value.filter((t) => selected.value.has(t.id)));
  let sequence = 0,
    alive = true,
    anchor: string | undefined;
  const active = () => alive && account().epoch === owner && account().status === 'authenticated';
  async function read(quiet = false) {
    const n = ++sequence;
    if (!quiet) loading.value = true;
    try {
      const result = await core.downloads(owner, queryAll);
      if (!active() || n !== sequence || result.epoch !== owner) return;
      if (result.tasks.length !== result.total)
        throw { code: 'invalid_response', message: '下载记录未能完整读取，请重试。' };
      records.value = result.tasks;
      migration.value = !!result.migration;
      const present = new Set(result.tasks.map((t) => t.id));
      selected.value = new Set([...selected.value].filter((id) => present.has(id)));
      if (anchor && !present.has(anchor)) anchor = undefined;
      error.value = '';
    } catch (e) {
      if (!active() || n !== sequence) return;
      const problem = failure(e);
      error.value = problem.message;
      if (['session_expired', 'profile_changed'].includes(problem.code)) expired();
    } finally {
      if (active() && n === sequence) loading.value = false;
    }
  }
  function toggle(index: number, range: boolean) {
    selected.value = toggleBatchSelection(
      ids.value,
      selected.value,
      index,
      anchor ? ids.value.indexOf(anchor) : -1,
      range,
    );
    if (!range) anchor = ids.value[index];
  }
  function select(index: number) {
    const id = ids.value[index];
    if (id) {
      selected.value = new Set([id]);
      anchor = id;
    }
  }
  function all(force = false) {
    selected.value =
      !force && selected.value.size === ids.value.length ? new Set() : new Set(ids.value);
    anchor = undefined;
  }
  function sort(column: BatchOrder['column']) {
    order.value = nextBatchOrder(order.value, column);
  }
  const changes = computed(() => {
    const v = downloads.view.value;
    return (
      v &&
      JSON.stringify([
        v.completedCount,
        v.migration?.phase,
        v.operations.map((o) => [o.id, o.phase, o.error]),
        v.jobs.map((j) => [j.id, j.running]),
      ])
    );
  });
  watch(changes, (now, old) => {
    if (now && old && now !== old && active()) void read(true);
  });
  const focused = () => {
    if (active()) void read(true);
  };
  onMounted(() => {
    void read();
    window.addEventListener('focus', focused);
  });
  onBeforeUnmount(() => {
    alive = false;
    sequence++;
    window.removeEventListener('focus', focused);
  });
  return {
    rows,
    chosen,
    selected,
    order,
    loading,
    error,
    migration,
    active,
    read,
    toggle,
    select,
    all,
    sort,
  };
}
