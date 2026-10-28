import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import {
  historyPending,
  type HistoryClient,
  type HistoryCommand,
  type HistoryKind,
  type HistoryPage,
} from '../services/history.ts';

export function useHistory(core: HistoryClient = useCore(), expired = () => {}) {
  const kind = ref<HistoryKind>('song'),
    page = shallowRef<HistoryPage>(),
    loading = ref(false),
    busy = ref(false),
    error = ref('');
  type Intent = Extract<HistoryCommand, { kind: 'mutate' }>;
  const unknown = shallowRef<Partial<Record<HistoryKind, Intent>>>({});
  const uncertain = computed(() => unknown.value[kind.value]);
  function remember(target: HistoryKind, intent?: Intent) {
    unknown.value = { ...unknown.value, [target]: intent };
  }
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  const pending = computed(
    () => !!uncertain.value || !!page.value?.operations.some(historyPending),
  );
  const canMutate = computed(
    () =>
      !!page.value &&
      !busy.value &&
      !loading.value &&
      !pending.value &&
      (!page.value.canSync || !page.value.syncEnabled || page.value.remoteFetchedAt !== null),
  );
  function connect(account: AccountView) {
    // Local playback remains available before login and after a 301. Native
    // account generations determine the guest namespace and remote permission.
    const owner = account.epoch;
    if (owner === epoch) return;
    epoch = owner;
    serial++;
    page.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    unknown.value = {};
  }
  function accept(result: AccountData<HistoryPage>, owner: string, target: HistoryKind) {
    if (
      result.epoch !== owner ||
      result.value.kind !== target ||
      result.value.entries.some((r) => r.resource.kind !== target)
    )
      throw { code: 'invalid_response', message: '播放历史响应不匹配，请重新读取' };
    page.value = result.value;
    if (
      unknown.value[target] &&
      result.value.operations.some((op) => op.id === unknown.value[target]?.id)
    )
      remember(target);
  }
  function failed(e: unknown) {
    const issue = failure(e);
    error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      page.value = undefined;
      unknown.value = {};
      expired();
      return true;
    }
    return false;
  }
  async function read(target: HistoryKind, remote: boolean, cache: boolean) {
    if (disposed || !epoch || busy.value) return;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && serial === current && epoch === owner;
    loading.value = true;
    error.value = '';
    try {
      if (cache) {
        try {
          const result = await core.historyPage(owner, target, false);
          if (!active()) return;
          accept(result, owner, target);
        } catch (e) {
          if (!active()) return;
          if (['session_expired', 'profile_changed'].includes(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      const result = await core.historyPage(owner, target, remote);
      if (active()) accept(result, owner, target);
    } catch (e) {
      if (active()) failed(e);
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function open(target: HistoryKind) {
    if (busy.value || disposed) return;
    if (kind.value !== target) {
      page.value = undefined;
    }
    kind.value = target;
    await read(target, true, true);
  }
  async function command(input: HistoryCommand) {
    if (disposed || !epoch || busy.value || loading.value) return false;
    if (input.kind === 'mutate' && !canMutate.value) return false;
    const owner = epoch,
      target = kind.value,
      current = ++serial;
    const active = () => !disposed && serial === current && epoch === owner;
    busy.value = true;
    error.value = '';
    try {
      const result = await core.historyCommand(owner, target, input);
      if (!active()) return false;
      accept(result, owner, target);
      return true;
    } catch (e) {
      if (!active()) return false;
      if (failed(e)) return false;
      if (input.kind === 'mutate') remember(target, input);
      // A response may be lost after the native intent was committed. Read its
      // journal; never manufacture a second UUID or automatically resend.
      try {
        const result = await core.historyPage(owner, target, false);
        if (active()) accept(result, owner, target);
      } catch {
        /* Keep the prior snapshot and pending intent until an explicit read. */
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  async function setSync(enabled: boolean) {
    if (pending.value) return;
    if (await command({ kind: 'set_sync', enabled })) await read(kind.value, enabled, false);
  }
  async function cancelUnknown() {
    const intent = uncertain.value;
    if (intent) await command({ ...intent, kind: 'cancel_unseen' });
  }
  function dispose() {
    disposed = true;
    serial++;
    page.value = undefined;
  }
  return {
    kind,
    page,
    loading,
    busy,
    error,
    pending,
    uncertain,
    canMutate,
    connect,
    open,
    command,
    setSync,
    cancelUnknown,
    dispose,
    refresh: () => read(kind.value, true, false),
    localRefresh: () => read(kind.value, false, false),
  };
}
export type HistoryModel = ReturnType<typeof useHistory>;
