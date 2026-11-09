import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import {
  LOCAL_BATCH_MAX_SELECTION,
  LOCAL_BATCH_PAGE_SIZE,
  localBatchQuery,
  type LocalBatchPlayback,
  type LocalClient,
  type LocalPage,
  type LocalQuery,
  type LocalTrackReference,
} from '../services/local.ts';

type Selected = LocalTrackReference & { ordinal: number };
const staleNotice = '本地列表已更新，已清空选择并返回第一页。';
const removedNotice = '已从列表中删除，源文件仍保留。重新扫描可恢复。';

/** One visit owns a flat query and may select only rows from one index revision. */
export function useLocalBatch(
  source: LocalQuery,
  core: Pick<LocalClient, 'localPage' | 'localCommand'> = useCore(),
  currentVisit = () => true,
  playSelection?: (selection: LocalBatchPlayback) => Promise<void>,
) {
  const query = localBatchQuery(source);
  const page = shallowRef<LocalPage>();
  const selected = shallowRef(new Map<string, Selected>());
  const loading = ref(false);
  const busyAction = ref<'remove' | 'play'>();
  const busy = computed(() => !!busyAction.value);
  const readError = ref(''),
    commandError = ref(''),
    notice = ref('');
  let failedOffset = 0;
  let disposed = false;
  const live = () => !disposed && currentVisit();
  const locked = computed(() => loading.value || busy.value || !!readError.value || !page.value);
  const selectionCount = computed(() => selected.value.size);
  const pageSelection = computed(() => {
    const rows = page.value?.tracks ?? [];
    const count = rows.filter(({ track }) => selected.value.has(track.id)).length;
    return { all: !!count && count === rows.length, some: count > 0 && count < rows.length };
  });

  function reset(noticeText: string) {
    selected.value = new Map();
    page.value = undefined;
    commandError.value = '';
    notice.value = noticeText;
  }

  function resetStale() {
    reset(notice.value === removedNotice ? `${removedNotice} ${staleNotice}` : staleNotice);
  }

  async function fetch(offset: number, revision: string | null): Promise<void> {
    failedOffset = offset;
    try {
      const response = await core.localPage({ ...query, offset, revision });
      if (!live()) return;
      if (revision && response.revision !== revision) {
        resetStale();
        await fetch(0, null);
        return;
      }
      // After a successful deletion, keep the current page when it still
      // exists, otherwise use the first page of the new revision.
      if (offset > 0 && offset >= response.total) {
        await fetch(0, response.revision);
        return;
      }
      page.value = response;
      readError.value = '';
    } catch (cause) {
      if (!live()) return;
      const issue = failure(cause);
      if (issue.code === 'snapshot_changed' && revision) {
        resetStale();
        await fetch(0, null);
      } else readError.value = issue.message;
    }
  }

  async function load(offset = 0) {
    if (!live() || busy.value || loading.value) return;
    loading.value = true;
    readError.value = '';
    try {
      await fetch(offset, page.value?.revision ?? null);
    } finally {
      if (live()) loading.value = false;
    }
  }
  function move(direction: -1 | 1) {
    const current = page.value;
    if (!current || locked.value || (direction < 0 ? current.offset === 0 : !current.more)) return;
    return load(current.offset + direction * LOCAL_BATCH_PAGE_SIZE);
  }
  function toggle(id?: string) {
    const current = page.value;
    if (!live() || locked.value || !current) return;
    const rows = current.tracks.flatMap((row, index) =>
      id === undefined || row.track.id === id ? [{ row, ordinal: current.offset + index }] : [],
    );
    if (!rows.length) return;
    const next = new Map(selected.value);
    const remove = rows.every(({ row }) => next.has(row.track.id));
    for (const { row, ordinal } of rows) {
      if (remove) next.delete(row.track.id);
      else next.set(row.track.id, { id: row.track.id, revision: row.revision, ordinal });
    }
    if (next.size > LOCAL_BATCH_MAX_SELECTION) {
      notice.value = '一次最多选择 50000 首。';
      return;
    }
    selected.value = next;
    commandError.value = notice.value = '';
  }

  const selectedReferences = () =>
    [...selected.value.values()]
      .sort((left, right) => left.ordinal - right.ordinal)
      .map(({ id, revision }) => ({ id, revision }));

  async function play() {
    const current = page.value;
    if (!playSelection || !live() || locked.value || !current || !selected.value.size) return;
    busyAction.value = 'play';
    commandError.value = notice.value = '';
    try {
      await playSelection({ query, revision: current.revision, tracks: selectedReferences() });
    } catch (cause) {
      if (!live()) return;
      const issue = failure(cause);
      if (issue.code === 'snapshot_changed') {
        resetStale();
        await fetch(0, null);
      } else commandError.value = `播放：${issue.message}`;
    } finally {
      if (live()) busyAction.value = undefined;
    }
  }

  async function remove() {
    const current = page.value;
    if (!live() || locked.value || !current || !selected.value.size) return;
    const tracks = selectedReferences();
    busyAction.value = 'remove';
    commandError.value = notice.value = '';
    try {
      await core.localCommand({ kind: 'hide_tracks', query, revision: current.revision, tracks });
      if (!live()) return;
      reset(removedNotice);
      await fetch(current.offset, null);
    } catch (cause) {
      if (!live()) return;
      const issue = failure(cause);
      if (issue.code === 'snapshot_changed') {
        resetStale();
        await fetch(0, null);
      } else commandError.value = `从列表中删除：${issue.message}`;
    } finally {
      if (live()) busyAction.value = undefined;
    }
  }
  return {
    query,
    page,
    selected,
    loading,
    busy,
    busyAction,
    locked,
    readError,
    commandError,
    notice,
    selectionCount,
    pageSelection,
    load,
    move,
    toggle,
    play,
    remove,
    retry: () => load(failedOffset),
    dispose: () => {
      disposed = true;
    },
  };
}
