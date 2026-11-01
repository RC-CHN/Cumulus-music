import { ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreFailure } from '../services/contracts.ts';
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
    reasonsIssue = shallowRef<CoreFailure>(),
    trashIssue = shallowRef<CoreFailure & { more: boolean }>(),
    stateIssue = shallowRef<CoreFailure>(),
    writeIssue = shallowRef<CoreFailure>(),
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
    if (visible && !busy.value && !loading.value && !stateIssue.value) void load(false);
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
    trashIssue.value = undefined;
    loading.value = false;
    busy.value = false;
    trashLoading.value = false;
    error.value = '';
    stateIssue.value = undefined;
    writeIssue.value = undefined;
    if (visible && epoch) void load(true);
  }
  function fail(e: unknown, report = true) {
    const issue = failure(e);
    if (report) error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      view.value = undefined;
      trash.value = undefined;
      clearReasons();
      expired();
    }
    return issue;
  }
  function clearReasons() {
    reasonSerial++;
    reasons.value = undefined;
    reasonsIssue.value = undefined;
    reasonsLoading.value = false;
  }
  async function loadReasons(entryId: string) {
    clearReasons();
    const owner = epoch,
      current = reasonSerial;
    if (!owner || disposed || busy.value || stateIssue.value) return;
    reasonsLoading.value = true;
    error.value = '';
    const active = () => !disposed && owner === epoch && current === reasonSerial;
    try {
      const r = await core.fmReasons(owner, source.value, entryId);
      if (active() && r.epoch === owner && r.value.entryId === entryId) reasons.value = r.value;
    } catch (e) {
      if (active()) reasonsIssue.value = fail(e);
    } finally {
      if (active()) reasonsLoading.value = false;
    }
  }
  async function load(refresh = false, afterWrite = false): Promise<boolean> {
    if (busy.value && !afterWrite) return false;
    const owner = epoch,
      current = ++serial;
    if (!owner || disposed) return false;
    const channel = source.value;
    if (refresh) error.value = '';
    loading.value = true;
    const active = () => !disposed && current === serial && owner === epoch;
    try {
      const r = await core.fmView(owner, channel, refresh);
      if (
        active() &&
        (r.epoch !== owner || JSON.stringify(r.value.source) !== JSON.stringify(channel))
      )
        throw { code: 'stale', message: '播放状态已更新，请重新读取', retryable: true };
      if (active() && r.epoch === owner) {
        if (followActive) {
          followActive = false;
          if (
            r.value.activeSource &&
            JSON.stringify(r.value.activeSource) !== JSON.stringify(source.value)
          ) {
            const next = r.value.activeSource;
            return await select(
              next,
              next.kind === 'zone'
                ? '专区漫游'
                : next.kind === 'familiar'
                  ? '相似歌曲'
                  : '私人漫游',
            );
          }
        }
        view.value = r.value;
        if (stateIssue.value) error.value = writeIssue.value?.message ?? '';
        stateIssue.value = undefined;
        return true;
      }
    } catch (e) {
      if (active()) {
        const issue = fail(e);
        if (active()) stateIssue.value = issue;
      }
    } finally {
      if (active()) loading.value = false;
    }
    return false;
  }
  async function command(command: FmCommand) {
    const owner = epoch;
    if (!owner || busy.value || disposed) return;
    // A failed observation cannot make a possibly sent write safe to repeat.
    // Only a fresh, account-bound state read releases this UI gate.
    if (stateIssue.value) return { ok: false, issue: stateIssue.value } as const;
    const current = ++writeSerial;
    serial++;
    loading.value = false;
    busy.value = true;
    error.value = '';
    writeIssue.value = undefined;
    if (command.kind === 'restore_trash' || command.kind === 'reconcile') {
      trashSerial++;
      trashLoading.value = false;
    }
    const active = () => !disposed && owner === epoch && current === writeSerial;
    try {
      const r = await core.fmCommand(owner, source.value, command);
      if (active() && r.epoch === owner) {
        view.value = r.value;
        return { ok: true, value: r.value } as const;
      }
    } catch (e) {
      if (active()) {
        const issue = fail(e);
        if (!active()) return;
        writeIssue.value = issue;
        await load(false, true);
        if (active()) return { ok: false, issue } as const;
      }
    } finally {
      if (active()) busy.value = false;
    }
  }
  async function loadTrash(more = false) {
    const owner = epoch;
    if (!owner || disposed || busy.value || trashLoading.value) return false;
    const old = more ? trash.value : undefined;
    if (more && !old?.more) return false;
    const current = ++trashSerial,
      page = old ? old.page + 1 : 1;
    trashLoading.value = true;
    error.value = '';
    trashIssue.value = undefined;
    const active = () => !disposed && owner === epoch && current === trashSerial;
    try {
      const r = await core.fmTrash(owner, page, old?.cursor ?? '');
      if (active() && (r.epoch !== owner || r.value.page !== page))
        throw { code: 'stale', message: '列表已更新，请重新读取', retryable: true };
      if (active()) {
        // A retry/overlapping page can repeat an ID. Keep its position and use
        // the latest metadata without manufacturing another selectable row.
        const songs = new Map((old?.songs ?? []).map((track) => [track.id, track]));
        for (const track of r.value.songs) songs.set(track.id, track);
        trash.value = { ...r.value, songs: [...songs.values()] };
        return true;
      }
    } catch (e) {
      if (active()) {
        const issue = fail(e, false);
        if (active()) trashIssue.value = { ...issue, more };
      }
    } finally {
      if (active()) trashLoading.value = false;
    }
    return false;
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
    writeIssue.value = undefined;
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
    trashIssue,
    reasons,
    reasonsIssue,
    stateIssue,
    writeIssue,
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
