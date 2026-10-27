import { ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
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
  const query = ref<DownloadQuery>({
    tab: 'completed',
    keyword: '',
    sort: 'completed',
    descending: true,
    offset: 0,
    limit: 100,
  });
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    readError = ref(''),
    notice = ref('');
  let epoch = '',
    generation = 0,
    read = 0,
    visible = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function issue(e: unknown, reading = false) {
    const v = failure(e);
    if (v.code !== 'stale') (reading ? readError : error).value = v.message;
    if (['session_expired', 'profile_changed'].includes(v.code)) sessionChanged();
  }
  function stopTimer() {
    clearTimeout(timer);
    timer = undefined;
  }
  async function refresh(quiet = false) {
    stopTimer();
    if (!epoch) return;
    const serial = ++read,
      captured = epoch,
      version = generation;
    if (!quiet) loading.value = true;
    try {
      const result = await core.downloads(captured, { ...query.value });
      if (version !== generation || serial !== read || result.epoch !== epoch) return;
      view.value = result;
      readError.value = '';
      if (result.offset >= result.total && result.offset > 0) {
        query.value.offset = Math.max(
          0,
          Math.floor(Math.max(0, result.total - 1) / query.value.limit) * query.value.limit,
        );
        await refresh(quiet);
      }
    } catch (e) {
      if (version === generation && serial === read) issue(e, true);
    } finally {
      if (version === generation && serial === read) {
        loading.value = false;
        if (visible) timer = setTimeout(() => void refresh(true), 850);
      }
    }
  }
  async function connect(account: AccountView) {
    generation++;
    read++;
    stopTimer();
    epoch = account.status === 'authenticated' ? account.epoch : '';
    view.value = undefined;
    error.value = '';
    readError.value = '';
    notice.value = '';
    busy.value = false;
    loading.value = false;
    query.value.offset = 0;
    await refresh();
  }
  function show(value: boolean) {
    visible = value;
    stopTimer();
    if (value) void refresh();
  }
  function filter(value: Partial<DownloadQuery>) {
    query.value = { ...query.value, offset: 0, ...value };
    void refresh();
  }
  async function command(input: DownloadCommand): Promise<DownloadResult | undefined> {
    if (!epoch || busy.value) return;
    const version = generation,
      captured = epoch;
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.downloadCommand(captured, input);
      if (version !== generation || result.epoch !== epoch) return;
      if (result.failed.length)
        notice.value = `已受理 ${result.accepted.length} 项，${result.failed.length} 项未完成：${result.failed[0]?.message}`;
      else
        notice.value = result.operationId
          ? '操作已开始，进度将自动保存'
          : result.accepted.length
            ? `已受理 ${result.accepted.length} 项`
            : '已保存';
      await refresh(true);
      return result;
    } catch (e) {
      if (version === generation) issue(e);
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
  }
  return {
    view,
    query,
    loading,
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
