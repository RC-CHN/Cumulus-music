import { computed, ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import { failure } from '../services/contracts.ts';
import {
  defaultLocalQuery,
  localQueryKey,
  type LocalClient,
  type LocalCommand,
  type LocalPage,
  type LocalQuery,
} from '../services/local.ts';
export function useLocal(core: LocalClient = useCore()) {
  const snapshot = shallowRef<LocalPage>();
  const snapshotKey = ref('');
  const query = ref(defaultLocalQuery());
  const view = computed(() =>
    snapshotKey.value === localQueryKey(query.value) ? snapshot.value : undefined,
  );
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    readError = ref(''),
    notice = ref('');
  let visible = false,
    disposed = false,
    read = 0,
    visit = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function stop() {
    clearTimeout(timer);
    timer = undefined;
  }
  function poll() {
    stop();
    if (visible && !disposed) timer = setTimeout(() => void refresh(true), 1500);
  }
  async function refresh(quiet = false) {
    if (disposed) return;
    stop();
    const serial = ++read,
      requested = { ...query.value };
    if (!quiet) loading.value = true;
    try {
      const value = await core.localPage(requested);
      if (disposed || serial !== read) return;
      snapshot.value = value;
      snapshotKey.value = localQueryKey(requested);
      readError.value = '';
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
      } else readError.value = issue.message;
    } finally {
      if (!disposed && serial === read) {
        loading.value = false;
        poll();
      }
    }
  }
  function show(value: boolean, refreshRevision = false) {
    if (visible !== value) visit++;
    visible = value;
    stop();
    if (value) {
      if (refreshRevision) query.value = { ...query.value, revision: null };
      void refresh();
    } else {
      read++;
      loading.value = false;
    }
  }
  function filter(value: Partial<LocalQuery>) {
    const changedGroup = value.group && value.group !== query.value.group;
    const next = {
      ...query.value,
      offset: 0,
      revision: null,
      ...(changedGroup ? { keyword: '', groupKey: null } : {}),
      ...value,
    };
    if (localQueryKey(next) !== localQueryKey(query.value)) {
      error.value = '';
      readError.value = '';
      notice.value = '';
    }
    query.value = next;
    void refresh();
  }
  function page(offset: number) {
    filter({ offset, revision: view.value?.revision ?? null });
  }
  async function command(input: LocalCommand) {
    if (busy.value || disposed) return;
    const commandVisit = visit;
    let commandQuery = localQueryKey(query.value);
    const listRemoval =
      input.kind === 'hide_track' || input.kind === 'hide_group' || input.kind === 'hide_tracks';
    const stillShowingCommand = () =>
      visit === commandVisit && localQueryKey(query.value) === commandQuery;
    busy.value = true;
    error.value = '';
    notice.value = '';
    stop();
    read++;
    loading.value = false;
    try {
      await core.localCommand(input);
      if (!disposed) {
        query.value.revision = null;
        await refresh(true);
        if (
          input.kind === 'hide_group' &&
          !disposed &&
          !readError.value &&
          stillShowingCommand() &&
          view.value &&
          query.value.groupKey !== null &&
          !view.value.groups.some((group) => group.key === query.value.groupKey)
        ) {
          query.value = { ...query.value, groupKey: view.value.groups[0]?.key ?? null, offset: 0 };
          commandQuery = localQueryKey(query.value);
          await refresh(true);
        }
        if (listRemoval && !disposed && !readError.value && stillShowingCommand())
          notice.value = '已从列表中删除，源文件仍保留。重新扫描可恢复。';
      }
    } catch (e) {
      if (!disposed && (!listRemoval || stillShowingCommand()))
        error.value = `${{ watch: '添加目录', enable: '恢复扫描', disable: '暂停扫描', rescan: '重新扫描', hide_track: '从列表中删除', hide_group: '从列表中删除', hide_tracks: '从列表中删除' }[input.kind]}：${failure(e).message}`;
    } finally {
      if (!disposed) {
        busy.value = false;
        poll();
      }
    }
  }
  async function choose() {
    if (busy.value || disposed) return;
    busy.value = true;
    error.value = '';
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
  function captureVisit() {
    const captured = visit;
    return () => visible && !disposed && captured === visit;
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
    show,
    filter,
    page,
    command,
    choose,
    openTrackDirectory: (id: string, revision: string) =>
      core.openLocalTrackDirectory(id, revision),
    revealFolder: (query: LocalQuery, revision: string) => core.revealLocalFolder(query, revision),
    captureVisit,
    dispose,
  };
}
export type LocalModel = ReturnType<typeof useLocal>;
