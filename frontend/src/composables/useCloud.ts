import { computed, ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import { failure, type AccountView } from '../services/contracts.ts';
import {
  deletionMessage,
  type CloudClient,
  type CloudPage,
  type CloudQuery,
  type CloudCommand,
  type CloudResult,
  type CloudTab,
  type CloudRemoval,
} from '../services/cloud.ts';
import type { CloudDeleteTarget } from '../services/cloudDelete.ts';
function defaultQuery(tab: CloudTab = 'uploaded'): CloudQuery {
  return {
    tab,
    keyword: '',
    sort: tab === 'uploaded' ? 'uploaded' : 'natural',
    descending: tab === 'uploaded',
    offset: 0,
    limit: 200,
  };
}
function queryKey(v: CloudQuery) {
  return JSON.stringify([v.tab, v.keyword, v.sort, v.descending, v.offset, v.limit]);
}
export function useCloud(core: CloudClient = useCore(), sessionChanged = () => {}) {
  const view = shallowRef<CloudPage>();
  const snapshotQuery = shallowRef<CloudQuery>();
  const query = ref<CloudQuery>(defaultQuery());
  const accountEpoch = ref('');
  const page = computed(() =>
    snapshotQuery.value && queryKey(snapshotQuery.value) === queryKey(query.value)
      ? view.value
      : undefined,
  );
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    readError = ref(''),
    notice = ref('');
  let saved: Partial<Record<CloudTab, CloudQuery>> = {};
  let epoch = '',
    generation = 0,
    read = 0,
    visible = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function stop() {
    clearTimeout(timer);
    timer = undefined;
  }
  function issue(e: unknown, reading = false) {
    const v = failure(e);
    if (v.code !== 'stale') (reading ? readError : error).value = v.message;
    if (['session_expired', 'profile_changed'].includes(v.code)) sessionChanged();
  }
  async function refresh(remote = false, quiet = false) {
    stop();
    if (!epoch) return;
    const serial = ++read,
      version = generation,
      captured = epoch,
      requested = { ...query.value };
    if (!quiet) loading.value = true;
    try {
      const value = await core.cloudPage(captured, requested, remote);
      if (version !== generation || serial !== read || value.epoch !== epoch) return;
      view.value = value;
      snapshotQuery.value = requested;
      if (remote) readError.value = '';
      if (value.offset >= value.total && value.offset > 0) {
        query.value.offset =
          Math.floor(Math.max(0, value.total - 1) / query.value.limit) * query.value.limit;
        await refresh(false, quiet);
      }
    } catch (e) {
      if (version === generation && serial === read) issue(e, true);
    } finally {
      if (version === generation && serial === read) {
        loading.value = false;
        if (visible && (view.value?.uploadingCount ?? 0) > 0)
          timer = setTimeout(() => void refresh(false, true), 850);
      }
    }
  }
  async function connect(account: AccountView) {
    generation++;
    read++;
    stop();
    epoch = account.status === 'authenticated' ? account.epoch : '';
    if (accountEpoch.value !== epoch) {
      saved = {};
      query.value = defaultQuery();
    }
    accountEpoch.value = epoch;
    view.value = undefined;
    snapshotQuery.value = undefined;
    error.value = '';
    readError.value = '';
    notice.value = '';
    loading.value = false;
    busy.value = false;
    query.value.offset = 0;
    if (visible) {
      const version = generation;
      await refresh();
      if (visible && version === generation) await refresh(true);
    }
  }
  async function show(value: boolean) {
    visible = value;
    stop();
    if (value) {
      const version = generation;
      await refresh();
      if (visible && version === generation) await refresh(true);
    }
  }
  function filter(value: Partial<CloudQuery>) {
    saved[query.value.tab] = { ...query.value };
    const base =
      value.tab && value.tab !== query.value.tab
        ? (saved[value.tab] ?? defaultQuery(value.tab))
        : { ...query.value, offset: 0 };
    const next = { ...base, ...value };
    if (queryKey(next) !== queryKey(query.value)) {
      notice.value = '';
      error.value = '';
    }
    query.value = next;
    void refresh();
  }
  async function command(input: CloudCommand): Promise<CloudResult | undefined> {
    if (!epoch || busy.value) return;
    const version = generation,
      tab = query.value.tab;
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.cloudCommand(epoch, input);
      if (version !== generation || result.epoch !== epoch) return;
      if (tab === query.value.tab)
        notice.value = result.deletion
          ? deletionMessage(result.deletion)
          : result.notStarted?.length
            ? `已登记 ${result.accepted.length} 个任务，${result.notStarted.length} 个尚未开始：${result.notStarted[0]?.message}`
            : result.failed.length
              ? `${result.accepted.length} 项已受理，${result.failed.length} 项未完成：${result.failed[0]?.message}`
              : `已受理 ${result.accepted.length} 项`;
      await refresh(false, true);
      return result;
    } catch (e) {
      if (version === generation) {
        if (tab === query.value.tab) issue(e);
        else if (['session_expired', 'profile_changed'].includes(failure(e).code)) sessionChanged();
      }
    } finally {
      if (version === generation) busy.value = false;
    }
  }
  async function pick(kind: 'audio' | 'folder' | 'cover' | 'lyric' | 'cue') {
    if (!epoch) return [];
    const version = generation;
    try {
      const paths = await core.pickCloudFiles(epoch, kind);
      return version === generation ? paths : [];
    } catch (e) {
      if (version === generation) issue(e);
      return [];
    }
  }
  async function removalCall(
    run: (captured: string) => Promise<CloudRemoval>,
    mutating = false,
  ): Promise<CloudRemoval | undefined> {
    if (!epoch || busy.value) return;
    const version = generation;
    busy.value = true;
    error.value = '';
    try {
      const result = await run(epoch);
      if (version !== generation || result.epoch !== epoch) return;
      if (mutating) {
        notice.value = result.error ?? (result.deletion ? deletionMessage(result.deletion) : '');
        await refresh(false, true);
      }
      return result;
    } catch (e) {
      if (version === generation) issue(e);
    } finally {
      if (version === generation) busy.value = false;
    }
  }
  function deletePrepare(target: CloudDeleteTarget) {
    if (target.epoch !== accountEpoch.value) return Promise.resolve(undefined);
    return removalCall((captured) =>
      core.cloudDeletePrepare(captured, [...target.ids], target.revision),
    );
  }
  function deleteRead(id: string) {
    return removalCall((captured) => core.cloudDeleteView(captured, id, true), true);
  }
  function deleteExecute(id: string, deleteLocal: boolean, retryLocal: boolean) {
    return removalCall(
      (captured) => core.cloudDeleteExecute(captured, id, deleteLocal, retryLocal),
      true,
    );
  }
  async function deleteDiscard(captured: string, id: string) {
    // Discard only unconfirmed native previews. Files and remote resources are untouched.
    if (captured !== epoch) return;
    try {
      await core.cloudDeleteDiscard(captured, id);
    } catch {
      /* Core bounds abandoned previews. */
    }
  }
  function dispose() {
    generation++;
    read++;
    stop();
    epoch = '';
  }
  return {
    view,
    page,
    accountEpoch,
    query,
    loading,
    busy,
    error,
    readError,
    notice,
    refresh,
    connect,
    show,
    filter,
    command,
    pick,
    deletePrepare,
    deleteRead,
    deleteExecute,
    deleteDiscard,
    dispose,
  };
}
export type CloudModel = ReturnType<typeof useCloud>;
