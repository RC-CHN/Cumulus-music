import { ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import { failure } from '../services/contracts.ts';
import {
  defaultLocalQuery,
  type LocalClient,
  type LocalCommand,
  type LocalPage,
  type LocalQuery,
} from '../services/local.ts';
export function useLocal(core: LocalClient = useCore()) {
  const view = shallowRef<LocalPage>();
  const query = ref(defaultLocalQuery());
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    notice = ref('');
  let visible = false,
    disposed = false,
    read = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function stop() {
    clearTimeout(timer);
    timer = undefined;
  }
  async function refresh(quiet = false) {
    if (disposed) return;
    stop();
    const serial = ++read;
    if (!quiet) loading.value = true;
    try {
      const value = await core.localPage({ ...query.value });
      if (disposed || serial !== read) return;
      view.value = value;
      error.value = '';
      if (value.offset >= value.total && value.offset > 0) {
        query.value.offset = 0;
        query.value.revision = null;
        await refresh(quiet);
      }
    } catch (e) {
      if (disposed || serial !== read) return;
      const issue = failure(e);
      if (issue.code === 'snapshot_changed' && query.value.revision) {
        query.value.offset = 0;
        query.value.revision = null;
        notice.value = '本地列表已更新，已返回第一页';
        await refresh(quiet);
      } else error.value = issue.message;
    } finally {
      if (!disposed && serial === read) {
        loading.value = false;
        if (visible) timer = setTimeout(() => void refresh(true), 1500);
      }
    }
  }
  function show(value: boolean) {
    visible = value;
    stop();
    if (value) void refresh();
    else {
      read++;
      loading.value = false;
    }
  }
  function filter(value: Partial<LocalQuery>) {
    query.value = { ...query.value, offset: 0, revision: null, ...value };
    void refresh();
  }
  function page(offset: number) {
    filter({ offset, revision: view.value?.revision ?? null });
  }
  async function command(input: LocalCommand) {
    if (busy.value || disposed) return;
    busy.value = true;
    error.value = '';
    notice.value = '';
    read++;
    try {
      await core.localCommand(input);
      if (!disposed) {
        query.value.revision = null;
        await refresh(true);
      }
    } catch (e) {
      if (!disposed) error.value = failure(e).message;
    } finally {
      if (!disposed) busy.value = false;
    }
  }
  async function choose() {
    if (busy.value || disposed) return;
    busy.value = true;
    let path: string | null = null;
    try {
      path = await core.pickLocalDirectory();
    } catch (e) {
      if (!disposed) error.value = failure(e).message;
    } finally {
      if (!disposed) busy.value = false;
    }
    if (path && !disposed) await command({ kind: 'watch', path });
  }
  function dispose() {
    disposed = true;
    read++;
    stop();
  }
  return {
    view,
    query,
    loading,
    busy,
    error,
    notice,
    refresh,
    show,
    filter,
    page,
    command,
    choose,
    dispose,
  };
}
export type LocalModel = ReturnType<typeof useLocal>;
