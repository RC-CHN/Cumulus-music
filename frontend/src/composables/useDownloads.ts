import { computed, ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { downloadCommandLabel } from '../services/downloadFeedback.ts';
import { downloadViewportKey, type DownloadViewport } from '../services/downloadViewport.ts';
import type {
  DownloadClient,
  DownloadCommand,
  DownloadQuery,
  DownloadResult,
  DownloadsView,
} from '../services/downloads.ts';

/** Polling projects native state. Transfers and recovery workers live in core. */
export function useDownloads(core: DownloadClient = useCore(), sessionChanged = () => {}) {
  const view = shallowRef<DownloadsView>();
  const viewport = shallowRef<DownloadViewport>();
  const initialQuery = (): DownloadQuery => ({
    tab: 'completed',
    category: 'song',
    keyword: '',
    sort: 'completed',
    descending: true,
    offset: 0,
    limit: 100,
  });
  const query = ref(initialQuery());
  const visit = shallowRef<object>();
  let visits = new WeakMap<
    object,
    { query: DownloadQuery; viewport?: DownloadViewport; moreError: string }
  >();
  const loading = ref(false),
    loadingMore = ref(false),
    moreError = ref(''),
    busy = ref(false),
    error = ref(''),
    readError = ref(''),
    notice = ref('');
  let epoch = '',
    generation = 0,
    read = 0,
    visible = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function issue(e: unknown, reading = false, context = '') {
    const v = failure(e);
    if (v.code !== 'stale')
      (reading ? readError : error).value = context ? `${context}：${v.message}` : v.message;
    if (['session_expired', 'profile_changed'].includes(v.code)) sessionChanged();
  }
  function stopTimer() {
    clearTimeout(timer);
    timer = undefined;
  }
  const hasMore = computed(() => !!view.value && view.value.tasks.length < view.value.total);
  function poll() {
    if (!visible) return;
    const active =
      query.value.tab === 'active' ||
      view.value?.activeCount ||
      view.value?.migration ||
      view.value?.jobs.some((job) => job.running);
    timer = setTimeout(() => void refresh(true), active ? 850 : 5000);
  }
  async function refresh(quiet = false) {
    stopTimer();
    if (!epoch) return;
    const serial = ++read,
      captured = epoch,
      version = generation;
    loadingMore.value = false;
    if (!quiet) loading.value = true;
    try {
      const result = await core.downloads(captured, { ...query.value });
      if (version !== generation || serial !== read || result.epoch !== epoch) return;
      view.value = result;
      readError.value = '';
    } catch (e) {
      if (version === generation && serial === read) issue(e, true);
    } finally {
      if (version === generation && serial === read) {
        loading.value = false;
        poll();
      }
    }
  }
  async function loadMore() {
    if (!epoch || !hasMore.value || loading.value || loadingMore.value || readError.value) return;
    stopTimer();
    const serial = ++read,
      version = generation,
      captured = epoch;
    const next = {
      ...query.value,
      offset: 0,
      limit: Math.min(view.value!.total, query.value.limit + 100),
    };
    loadingMore.value = true;
    moreError.value = '';
    try {
      // Native returns a fresh whole prefix, never append pages whose contents
      // may have shifted due to completion, deletion or a renamed file.
      const result = await core.downloads(captured, next);
      if (version !== generation || serial !== read || result.epoch !== epoch) return;
      query.value = next;
      view.value = result;
      readError.value = '';
    } catch (e) {
      if (version === generation && serial === read) {
        const problem = failure(e);
        if (problem.code !== 'stale') moreError.value = problem.message;
        if (['session_expired', 'profile_changed'].includes(problem.code)) sessionChanged();
      }
    } finally {
      if (version === generation && serial === read) {
        loadingMore.value = false;
        poll();
      }
    }
  }
  async function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : '';
    if (next && epoch === next) {
      await refresh();
      return;
    }
    generation++;
    read++;
    stopTimer();
    epoch = next;
    view.value = undefined;
    viewport.value = undefined;
    visits = new WeakMap();
    error.value = '';
    readError.value = '';
    notice.value = '';
    busy.value = false;
    loading.value = false;
    loadingMore.value = false;
    moreError.value = '';
    query.value = initialQuery();
    await refresh();
  }
  function show(value: boolean) {
    visible = value;
    stopTimer();
    if (value) void refresh();
  }
  function filter(value: Partial<DownloadQuery>) {
    const next = { ...query.value, ...value, offset: 0, limit: 100 };
    if (next.tab === 'active') next.category = 'all';
    else if (!next.category || next.category === 'all') next.category = 'song';
    if (JSON.stringify(next) === JSON.stringify(query.value)) return Promise.resolve();
    query.value = next;
    viewport.value = undefined;
    // Old rows must not acquire the new tab's actions while its read is pending.
    view.value = undefined;
    readError.value = '';
    moreError.value = '';
    return refresh();
  }
  async function command(input: DownloadCommand): Promise<DownloadResult | undefined> {
    if (!epoch || busy.value) return;
    const version = generation,
      captured = epoch;
    const label = downloadCommandLabel(input);
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.downloadCommand(captured, input);
      if (version !== generation || result.epoch !== epoch) return;
      if (input.kind === 'delete_files')
        notice.value =
          `已删除 ${result.accepted.length} 项` +
          (result.failed.length
            ? `，${result.failed.length} 项未完成：${result.failed[0]?.message}`
            : '');
      else if (result.failed.length)
        notice.value = `已受理 ${result.accepted.length} 项，${result.failed.length} 项未完成：${result.failed[0]?.message}`;
      else
        notice.value = result.operationId
          ? '操作已开始，进度将自动保存'
          : result.accepted.length
            ? `已受理 ${result.accepted.length} 项`
            : '已保存';
      notice.value = `${label}：${notice.value}`;
      await refresh(true);
      return result;
    } catch (e) {
      if (version === generation) issue(e, false, label);
    } finally {
      if (version === generation) busy.value = false;
    }
  }
  async function pick() {
    const version = generation;
    if (!epoch) return;
    try {
      const path = await core.pickDownloadDirectory(epoch);
      if (version === generation) return path ?? undefined;
    } catch (e) {
      if (version === generation) issue(e);
    }
  }
  async function open(taskId: string | null = null) {
    const version = generation;
    if (!epoch) return;
    try {
      await core.openDownloadDirectory(epoch, taskId);
    } catch (e) {
      if (version === generation) issue(e);
    }
  }
  function dispose() {
    generation++;
    read++;
    stopTimer();
    epoch = '';
    viewport.value = undefined;
    visits = new WeakMap();
    visit.value = undefined;
  }
  function saveViewport(value: DownloadViewport, owner: object | undefined = visit.value) {
    if (
      epoch &&
      owner === visit.value &&
      value.epoch === epoch &&
      value.query === downloadViewportKey(query.value)
    )
      viewport.value = value;
  }
  function leaveVisit() {
    if (visit.value && epoch)
      visits.set(visit.value, {
        query: { ...query.value },
        viewport: viewport.value,
        moreError: moreError.value,
      });
    visit.value = undefined;
  }
  function enterVisit(target: object, initial?: Partial<DownloadQuery>) {
    if (visit.value === target) return;
    leaveVisit();
    const saved = visits.get(target),
      previous = downloadViewportKey(query.value);
    visit.value = target;
    if (saved) {
      query.value = { ...saved.query };
      viewport.value = saved.viewport;
    } else if (initial) {
      query.value = { ...query.value, ...initial, offset: 0, limit: 100 };
      if (query.value.tab === 'active') query.value.category = 'all';
      viewport.value = undefined;
    }
    // Superseded reads may finish, but cannot update the newly selected visit.
    read++;
    stopTimer();
    loading.value = false;
    loadingMore.value = false;
    moreError.value = saved?.moreError ?? '';
    if (previous !== downloadViewportKey(query.value)) {
      view.value = undefined;
      readError.value = '';
    }
  }
  function captureVisit() {
    const captured = generation,
      owner = visit.value;
    return () => !!epoch && captured === generation && owner === visit.value;
  }
  return {
    view,
    viewport,
    saveViewport,
    visit,
    enterVisit,
    leaveVisit,
    captureVisit,
    query,
    loading,
    loadingMore,
    moreError,
    hasMore,
    loadMore,
    busy,
    error,
    readError,
    notice,
    connect,
    refresh,
    show,
    filter,
    command,
    pick,
    open,
    dispose,
  };
}
export type DownloadModel = ReturnType<typeof useDownloads>;
