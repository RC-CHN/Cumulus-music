import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure, type AccountData } from '../services/contracts.ts';
import type { HistorySyncClient, HistorySyncState } from '../services/history.ts';

export function useHistorySyncPreference(
  core: HistorySyncClient,
  owner: () => string | null,
  expired: () => void,
) {
  const value = shallowRef<HistorySyncState>();
  const loading = ref(false);
  const error = ref('');
  const confirmOff = ref(false);
  const saving = ref(false);
  const saveError = ref('');
  const saved = ref(false);
  const state = computed(() =>
    !owner() ? 'signed-out' : loading.value ? 'loading' : error.value ? 'error' : 'ready',
  );
  let visit = 0;
  let disposed = false;
  let listening = false;
  let unlisten: (() => void) | undefined;

  function accept(next: AccountData<HistorySyncState>) {
    if (disposed || next.epoch !== owner() || !/^(0|[1-9][0-9]{0,19})$/.test(next.value.revision))
      return;
    const current = value.value;
    if (current && BigInt(next.value.revision) <= BigInt(current.revision)) return;
    value.value = next.value;
    error.value = '';
  }

  async function refresh() {
    const epoch = owner();
    if (!epoch || disposed || !listening || loading.value) return;
    const generation = visit;
    const active = () => !disposed && visit === generation && owner() === epoch;
    const previous = value.value;
    error.value = '';
    loading.value = true;
    try {
      const next = await core.historySyncPreference(epoch, { kind: 'read' });
      if (!active()) return;
      if (next.epoch !== epoch) throw { code: 'stale' };
      accept(next);
      if (!value.value) throw { code: 'invalid_response' };
    } catch (cause) {
      // A newer native observation may already have completed the read.
      if (!active() || value.value !== previous) return;
      if (['session_expired', 'profile_changed', 'stale'].includes(failure(cause).code)) {
        error.value = '账号状态已变化，请重新登录后查看。';
        expired();
      } else error.value = '暂时无法读取，请重试。';
    } finally {
      if (active()) loading.value = false;
    }
  }

  async function save(enabled: boolean, confirmedFirstDisable: boolean) {
    const epoch = owner();
    if (!epoch || disposed || saving.value || !value.value || state.value !== 'ready') return;
    const generation = visit;
    const active = () => !disposed && visit === generation && owner() === epoch;
    saved.value = false;
    saveError.value = '';
    saving.value = true;
    try {
      const next = await core.historySyncPreference(epoch, {
        kind: 'set',
        enabled,
        confirmed_first_disable: confirmedFirstDisable,
      });
      if (!active()) return;
      if (next.epoch !== epoch || next.value.enabled !== enabled)
        throw { code: 'invalid_response', message: '设置结果不匹配，请重新读取。' };
      accept(next);
      saved.value = true;
    } catch (cause) {
      if (!active()) return;
      saveError.value = failure(cause).message;
      // A missing acknowledgement could have committed locally. Read only.
      await refresh();
    } finally {
      if (active()) {
        saving.value = false;
        confirmOff.value = false;
      }
    }
  }

  function prepareChange(): boolean | undefined {
    if (!value.value || saving.value || state.value !== 'ready') return;
    if (value.value.enabled && !value.value.firstDisableConfirmed) {
      confirmOff.value = true;
      return;
    }
    return !value.value.enabled;
  }

  watch(
    owner,
    () => {
      visit++;
      value.value = undefined;
      error.value = '';
      loading.value = false;
      confirmOff.value = false;
      saving.value = false;
      saved.value = false;
      saveError.value = '';
      if (listening) void refresh();
    },
    { flush: 'sync' },
  );

  async function start() {
    try {
      const stop = await core.onHistorySyncChanged(accept);
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
    } catch {
      // The one local snapshot still works if event subscription is unavailable.
    }
    if (disposed) return;
    listening = true;
    await refresh();
  }
  void start();
  onScopeDispose(() => {
    disposed = true;
    visit++;
    unlisten?.();
  });

  return {
    value,
    state,
    error,
    refresh,
    saving,
    saved,
    saveError,
    confirmOff,
    save,
    prepareChange,
  };
}
