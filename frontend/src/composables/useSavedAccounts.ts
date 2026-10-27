import { ref, shallowRef } from 'vue';
import type { AuthClient, SavedAccount } from '../services/auth.ts';
import { failure } from '../services/contracts.ts';

export function useSavedAccounts(
  core: Pick<AuthClient, 'savedAccounts' | 'removeSavedAccount'>,
  epoch: string,
) {
  const accounts = shallowRef<SavedAccount[]>([]);
  const loading = ref(true),
    removing = ref(false),
    error = ref('');
  let generation = 0,
    disposed = false;
  const current = (serial: number) => !disposed && serial === generation;
  async function load() {
    if (disposed || removing.value) return;
    const serial = ++generation;
    loading.value = true;
    error.value = '';
    try {
      const response = await core.savedAccounts(epoch);
      if (current(serial) && response.epoch === epoch) accounts.value = response.value;
    } catch (e) {
      if (current(serial)) error.value = failure(e).message;
    } finally {
      if (current(serial)) loading.value = false;
    }
  }
  async function remove(account: SavedAccount) {
    if (disposed || removing.value || account.current) return false;
    const serial = ++generation;
    removing.value = true;
    loading.value = false;
    error.value = '';
    let acknowledged = false;
    try {
      await core.removeSavedAccount(epoch, account.id);
      acknowledged = true;
      if (current(serial)) accounts.value = accounts.value.filter((a) => a.id !== account.id);
    } catch (e) {
      if (current(serial)) error.value = failure(e).message;
    }
    // Always read back local recovery state, including after a lost IPC reply.
    // A pending row stays visible; no automatic removal retry is issued.
    if (current(serial)) {
      try {
        const response = await core.savedAccounts(epoch);
        if (current(serial) && response.epoch === epoch) {
          accounts.value = response.value;
          if (!accounts.value.some((a) => a.id === account.id)) {
            acknowledged = true;
            error.value = '';
          }
        }
      } catch (e) {
        if (current(serial))
          error.value ||= acknowledged ? '账号已移除，列表读取失败，请刷新' : failure(e).message;
      }
    }
    if (current(serial)) removing.value = false;
    return current(serial) && acknowledged;
  }
  function dispose() {
    disposed = true;
    generation++;
  }
  return { accounts, loading, removing, error, load, remove, dispose };
}
