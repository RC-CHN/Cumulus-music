import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { FmClient, FmCommand, FmSource, FmTrash, FmView, FmReasons } from '../services/fm.ts';
import { useCore } from '../services/core.ts';
import { useFmDiscovery } from './useFmDiscovery.ts';
export function useFm(core: FmClient = useCore(), expired = () => {}) {
  const discovery = useFmDiscovery(core, expired);
  const title = ref('私人漫游');
  const source = shallowRef<FmSource>({ kind: 'personal' }),
    view = shallowRef<FmView>();
  const trash = shallowRef<FmTrash>(),
    reasons = shallowRef<FmReasons>(),
    reasonsLoading = ref(false),
    loading = ref(false),
    busy = ref(false),
    trashLoading = ref(false),
    error = ref('');
  let epoch: string | undefined,
    serial = 0,
    trashSerial = 0,
    writeSerial = 0,
    reasonSerial = 0,
    followActive = true,
    visible = false,
    disposed = false;
  const timer = setInterval(() => {
    if (visible && !busy.value && !loading.value) void load(false);
  }, 1200);
  function connect(account: AccountView) {
    discovery.connect(account);
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    source.value = { kind: 'personal' };
    title.value = '私人漫游';
    followActive = true;
    serial++;
    trashSerial++;
    writeSerial++;
    clearReasons();
    view.value = undefined;
    trash.value = undefined;
    loading.value = false;
    busy.value = false;
    trashLoading.value = false;
    error.value = '';
    if (visible && epoch) void load(true);
  }
  function fail(e: unknown) {
    const issue = failure(e);
    error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      view.value = undefined;
      trash.value = undefined;
      clearReasons();
      expired();
    }
  }
  function clearReasons() {
    reasonSerial++;
    reasons.value = undefined;
    reasonsLoading.value = false;
  }
  async function loadReasons(entryId: string) {
    clearReasons();
    const owner = epoch,
      current = reasonSerial;
    if (!owner || disposed || busy.value) return;
    reasonsLoading.value = true;
    error.value = '';
    const active = () => !disposed && owner === epoch && current === reasonSerial;
    try {
      const r = await core.fmReasons(owner, source.value, entryId);
      if (active() && r.epoch === owner && r.value.entryId === entryId) reasons.value = r.value;
    } catch (e) {
      if (active()) fail(e);
    } finally {
      if (active()) reasonsLoading.value = false;
    }
  }
  async function load(refresh = false, afterWrite = false) {
    if (busy.value && !afterWrite) return;
    const owner = epoch,
      current = ++serial;
    if (!owner || disposed) return;
    if (refresh) error.value = '';
    loading.value = true;
    const active = () => !disposed && current === serial && owner === epoch;
    try {
      const r = await core.fmView(owner, source.value, refresh);
      if (active() && r.epoch === owner) {
        if (followActive) {
          followActive = false;
          if (
            r.value.activeSource &&
            JSON.stringify(r.value.activeSource) !== JSON.stringify(source.value)
          ) {
            const next = r.value.activeSource;
            await select(
              next,
              next.kind === 'zone'
                ? '专区漫游'
                : next.kind === 'familiar'
                  ? '相似歌曲'
                  : '私人漫游',
            );
            return;
          }
        }
        view.value = r.value;
      }
    } catch (e) {
      if (active()) fail(e);
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function command(command: FmCommand) {
    const owner = epoch;
    if (!owner || busy.value || disposed) return;
    const current = ++writeSerial;
    serial++;
    loading.value = false;
    busy.value = true;
    error.value = '';
    if (command.kind === 'restore_trash' || command.kind === 'reconcile') {
      trashSerial++;
      trashLoading.value = false;
    }
    const active = () => !disposed && owner === epoch && current === writeSerial;
    try {
      const r = await core.fmCommand(owner, source.value, command);
      if (active() && r.epoch === owner) view.value = r.value;
    } catch (e) {
      if (active()) {
        fail(e);
        await load(false, true);
      }
    } finally {
      if (active()) busy.value = false;
    }
  }
  async function loadTrash(more = false) {
    const owner = epoch;
    if (!owner || disposed || trashLoading.value) return;
    const current = ++trashSerial,
      old = more ? trash.value : undefined;
    trashLoading.value = true;
    error.value = '';
    if (!more) trash.value = undefined;
    const active = () => !disposed && owner === epoch && current === trashSerial;
    try {
      const r = await core.fmTrash(owner, old ? old.page + 1 : 1, old?.cursor ?? '');
      if (active() && r.epoch === owner)
        trash.value = { ...r.value, songs: old ? [...old.songs, ...r.value.songs] : r.value.songs };
    } catch (e) {
      if (active()) fail(e);
    } finally {
      if (active()) trashLoading.value = false;
    }
  }
  function show(value: boolean) {
    if (value && !visible) followActive = true;
    visible = value;
    if (value) void load(true);
  }
  async function select(next: FmSource, name: string) {
    if (busy.value || disposed) return false;
    const owner = epoch;
    followActive = false;
    serial++;
    clearReasons();
    view.value = undefined;
    loading.value = false;
    source.value = next;
    title.value = name;
    await load(true);
    return !disposed && owner === epoch && source.value === next && !error.value && hasSource(next);
  }
  function hasSource(next: FmSource) {
    return JSON.stringify(view.value?.source) === JSON.stringify(next);
  }
  function dispose() {
    disposed = true;
    serial++;
    trashSerial++;
    writeSerial++;
    clearReasons();
    clearInterval(timer);
    discovery.dispose();
  }
  return {
    discovery,
    title,
    select,
    source,
    view,
    trash,
    reasons,
    reasonsLoading,
    clearReasons,
    loadReasons,
    loading,
    busy,
    trashLoading,
    error,
    connect,
    show,
    load,
    command,
    loadTrash,
    dispose,
  };
}
export type FmModel = ReturnType<typeof useFm>;
