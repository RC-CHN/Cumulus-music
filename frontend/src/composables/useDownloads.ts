import { computed, ref, shallowRef } from 'vue';
import type { AccountView, CoreFailure } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { downloadAdmissionSummary, downloadCommandLabel } from '../services/downloadFeedback.ts';
import { downloadViewportKey, type DownloadViewport } from '../services/downloadViewport.ts';
import type {
  DownloadClearPlan,
  DownloadClient,
  DownloadCommand,
  DownloadQuery,
  DownloadResult,
  DownloadsView,
  VoiceDownloadPreparation,
} from '../services/downloads.ts';

export type DownloadCommandObservation =
  | { result: DownloadResult; readback?: DownloadsView }
  | { failure: CoreFailure };

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
    return v;
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
  async function refresh(quiet = false): Promise<DownloadsView | undefined> {
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
      return result;
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
    const quiet = visible && !!view.value;
    visible = value;
    stopTimer();
    // Opening/closing an overlay on an already visible page must not disable
    // the control to which its leave transition is returning keyboard focus.
    if (value) void refresh(quiet);
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
  async function qualityPreferences(refresh = false) {
    if (!epoch) return;
    const captured = epoch,
      version = generation;
    try {
      const result = await core.downloadQualityPreferences(captured, refresh);
      if (version === generation && result.epoch === captured) return result;
    } catch (value) {
      if (version === generation) throw value;
    }
  }
  async function discardVoice(plan: VoiceDownloadPreparation) {
    if (!plan.receipt) return;
    try {
      await core.downloadVoiceDiscard(plan.epoch, plan.receipt);
    } catch {
      // Another visit may have retired this exact receipt.
    }
  }
  async function prepareScopedVoice(prepare: (epoch: string) => Promise<VoiceDownloadPreparation>) {
    if (!epoch) return;
    const captured = epoch,
      current = captureVisit();
    try {
      const plan = await prepare(captured);
      if (plan.epoch !== captured) return;
      if (current()) return plan;
      void discardVoice(plan);
    } catch (value) {
      if (current()) {
        if (['session_expired', 'profile_changed'].includes(failure(value).code)) sessionChanged();
        throw value;
      }
    }
  }
  function prepareVoice(
    voiceIds: string[],
    purchased?: import('../services/podcast.ts').PodcastPurchasedTarget,
    liked?: import('../services/podcast.ts').PodcastLikedProgramTarget,
    pagedLiked?: import('../services/podcast.ts').PodcastLikedPageTarget,
    history?: import('../services/history.ts').HistorySelectionScope,
  ) {
    return prepareScopedVoice((owner) =>
      history
        ? core.downloadHistoryVoicePrepare(owner, history)
        : pagedLiked
          ? core.downloadLikedPageVoicePrepare(owner, pagedLiked)
          : purchased
            ? core.downloadPurchasedVoicePrepare(owner, purchased)
            : liked
              ? core.downloadLikedVoicePrepare(owner, liked)
              : core.downloadVoicePrepare(owner, [...voiceIds]),
    );
  }
  async function discardClear(plan: DownloadClearPlan) {
    if (!plan.receipt) return;
    try {
      await core.downloadClearDiscard(plan.epoch, plan.receipt);
    } catch {
      // Another visit, account or confirmation may already have retired it.
    }
  }
  async function prepareClear() {
    if (!epoch) return;
    const captured = epoch,
      current = captureVisit();
    try {
      const plan = await core.downloadClearPrepare(captured);
      if (plan.epoch !== captured) return;
      if (current()) return plan;
      void discardClear(plan);
    } catch (value) {
      if (current()) throw value;
    }
  }
  async function observeCommand(
    input: DownloadCommand,
    expectedEpoch?: string,
  ): Promise<DownloadCommandObservation | undefined> {
    if (!epoch || busy.value || (expectedEpoch !== undefined && expectedEpoch !== epoch)) return;
    const version = generation,
      captured = epoch;
    const label = downloadCommandLabel(input);
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.downloadCommand(captured, input);
      if (version !== generation || result.epoch !== epoch) return;
      const admissionSummary = downloadAdmissionSummary(result);
      if (admissionSummary) notice.value = admissionSummary;
      else if (input.kind === 'delete_files')
        notice.value =
          `已删除 ${result.accepted.length} 项` +
          (result.failed.length
            ? `，${result.failed.length} 项未完成：${result.failed[0]?.message}`
            : '');
      else if (input.kind === 'confirm_clear' && !result.accepted.length && !result.failed.length)
        notice.value = '确认范围内已没有需要取消的任务';
      else if (result.failed.length)
        notice.value = `已受理 ${result.accepted.length} 项，${result.failed.length} 项未完成：${result.failed[0]?.message}`;
      else
        notice.value = result.operationId
          ? '操作已开始，进度将自动保存'
          : result.accepted.length
            ? `已受理 ${result.accepted.length} 项`
            : '已保存';
      notice.value = `${label}：${notice.value}`;
      const readback = await refresh(true);
      if (version === generation && captured === epoch) return { result, readback };
    } catch (e) {
      if (version === generation) return { failure: issue(e, false, label) };
    } finally {
      if (version === generation) busy.value = false;
    }
  }
  async function command(input: DownloadCommand, expectedEpoch?: string) {
    const observation = await observeCommand(input, expectedEpoch);
    return observation && 'result' in observation ? observation.result : undefined;
  }
  function changeDirectory(
    input: Extract<DownloadCommand, { kind: 'change_directory' }>,
    expectedEpoch: string,
  ) {
    return observeCommand(input, expectedEpoch);
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
    qualityPreferences,
    prepareVoice,
    prepareScopedVoice,
    discardVoice,
    prepareClear,
    discardClear,
    refresh,
    show,
    filter,
    command,
    changeDirectory,
    pick,
    open,
    dispose,
  };
}
export type DownloadModel = ReturnType<typeof useDownloads>;
