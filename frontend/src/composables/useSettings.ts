import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import {
  settingsPending,
  type AccountSettingsCommand,
  type AccountSettingsPage,
  type SettingsClient,
  type SettingChange,
} from '../services/settings.ts';
export function useSettings(core: SettingsClient = useCore(), expired = () => {}) {
  const page = shallowRef<AccountSettingsPage>(),
    loading = ref(false),
    busy = ref(false),
    error = ref('');
  type Intent = Extract<AccountSettingsCommand, { change: SettingChange }>;
  const unknown = shallowRef<Intent>();
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  const pending = computed(() => !!unknown.value || !!page.value?.operations.some(settingsPending));
  const canChange = computed(
    () =>
      !!epoch &&
      !!page.value?.revision &&
      !busy.value &&
      !loading.value &&
      !pending.value &&
      !error.value,
  );
  function connect(account: AccountView) {
    const owner = account.status === 'authenticated' ? account.epoch : undefined;
    if (owner === epoch) return;
    epoch = owner;
    serial++;
    page.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    unknown.value = undefined;
  }
  function accept(result: AccountData<AccountSettingsPage>, owner: string) {
    if (result.epoch !== owner)
      throw { code: 'invalid_response', message: '账号设置响应不匹配，请重新读取' };
    page.value = result.value;
    if (unknown.value && result.value.operations.some((op) => op.id === unknown.value?.id))
      unknown.value = undefined;
  }
  function failed(e: unknown) {
    const issue = failure(e);
    error.value = issue.message;
    if (['session_expired', 'profile_changed', 'stale'].includes(issue.code)) {
      page.value = undefined;
      unknown.value = undefined;
      epoch = undefined;
      busy.value = false;
      loading.value = false;
      expired();
      return true;
    }
    return false;
  }
  async function read(refresh: boolean, cache = false) {
    if (disposed || !epoch || busy.value) return;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && owner === epoch && serial === current;
    loading.value = true;
    error.value = '';
    try {
      if (cache) {
        try {
          const result = await core.accountSettingsPage(owner, false);
          if (active()) accept(result, owner);
        } catch (e) {
          if (['session_expired', 'profile_changed', 'stale'].includes(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      const result = await core.accountSettingsPage(owner, refresh);
      if (active()) accept(result, owner);
    } catch (e) {
      if (active()) failed(e);
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function command(command: AccountSettingsCommand) {
    if (disposed || !epoch || busy.value || loading.value) return false;
    if (command.kind === 'change' && !canChange.value) return false;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && owner === epoch && serial === current;
    busy.value = true;
    error.value = '';
    try {
      const result = await core.accountSettingsCommand(owner, command);
      if (!active()) return false;
      accept(result, owner);
      return true;
    } catch (e) {
      if (!active() || failed(e)) return false;
      // These native errors precede any write or intent registration. Other
      // failures require journal inspection, including lost IPC replies.
      if (
        command.kind === 'change' &&
        !['settings_changed', 'setting_unavailable', 'invalid_input'].includes(failure(e).code)
      )
        unknown.value = command;
      try {
        const result = await core.accountSettingsPage(owner, false);
        if (active()) accept(result, owner);
      } catch {
        /* Retain uncertain intent. */
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  async function change(value: SettingChange) {
    if (!page.value?.revision || !canChange.value) return false;
    return command({
      kind: 'change',
      id: crypto.randomUUID(),
      expected_revision: page.value.revision,
      change: value,
    });
  }
  async function cancelUnknown() {
    if (unknown.value) await command({ ...unknown.value, kind: 'cancel_unseen' });
  }
  function dispose() {
    disposed = true;
    serial++;
    page.value = undefined;
    unknown.value = undefined;
  }
  return {
    page,
    loading,
    busy,
    error,
    unknown,
    pending,
    canChange,
    connect,
    command,
    change,
    cancelUnknown,
    dispose,
    open: () => read(true, true),
    refresh: () => read(true),
    localRefresh: () => read(false),
  };
}
export type SettingsModel = ReturnType<typeof useSettings>;
