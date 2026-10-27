import { ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import { failure, type AccountView } from '../services/contracts.ts';
import {
  deletionMessage,
  type CloudClient,
  type CloudPage,
  type CloudQuery,
  type CloudCommand,
  type CloudResult,
} from '../services/cloud.ts';
export function useCloud(core: CloudClient = useCore(), sessionChanged = () => {}) {
  const view = shallowRef<CloudPage>();
  const query = ref<CloudQuery>({
    tab: 'uploaded',
    keyword: '',
    sort: 'uploaded',
    descending: true,
    offset: 0,
    limit: 200,
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
      captured = epoch;
    if (!quiet) loading.value = true;
    try {
      const value = await core.cloudPage(captured, { ...query.value }, remote);
      if (version !== generation || serial !== read || value.epoch !== epoch) return;
      view.value = value;
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
    view.value = undefined;
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
    query.value = { ...query.value, offset: 0, ...value };
    void refresh();
  }
  async function command(input: CloudCommand): Promise<CloudResult | undefined> {
    if (!epoch || busy.value) return;
    const version = generation;
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.cloudCommand(epoch, input);
      if (version !== generation || result.epoch !== epoch) return;
      notice.value = result.deletion
        ? deletionMessage(result.deletion)
        : result.failed.length
          ? `${result.accepted.length} 项已受理，${result.failed.length} 项未完成：${result.failed[0]?.message}`
          : `已受理 ${result.accepted.length} 项`;
      await refresh(false, true);
      return result;
    } catch (e) {
      if (version === generation) issue(e);
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
  function dispose() {
    generation++;
    read++;
    stop();
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
    refresh,
    connect,
    show,
    filter,
    command,
    pick,
    dispose,
  };
}
export type CloudModel = ReturnType<typeof useCloud>;
