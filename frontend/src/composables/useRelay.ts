import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import type { Quality } from '../services/player.ts';
import {
  isRelaySettingCommand,
  relaySettingPending,
  type RelaySettingCommand,
  type RelayClient,
  type RelayCommand,
  type RelayView,
} from '../services/relay.ts';
import { useCore } from '../services/core.ts';
export function useRelay(core: RelayClient = useCore(), expired = () => {}) {
  const page = shallowRef<RelayView>(),
    loading = ref(false),
    busy = ref(false),
    error = ref('');
  const hidden = ref('');
  const unknown = shallowRef<{ id: string; revision: string }>();
  const settingUnknown = shallowRef<Extract<RelaySettingCommand, { enabled: boolean }>>();
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  const alert = computed(() =>
    page.value?.revision !== hidden.value ? page.value?.alert : undefined,
  );
  const canChangeSetting = computed(
    () =>
      !!epoch &&
      !!page.value?.settings?.canChange &&
      !busy.value &&
      !loading.value &&
      !error.value &&
      !settingUnknown.value,
  );
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return false;
    epoch = next;
    serial++;
    page.value = undefined;
    hidden.value = '';
    unknown.value = undefined;
    settingUnknown.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    return true;
  }
  function accept(result: AccountData<RelayView>, owner: string) {
    if (result.epoch !== owner)
      throw { code: 'invalid_response', message: '接续账号已变化，请重新读取' };
    page.value = result.value;
    if (unknown.value && result.value.operations.some((r) => r.id === unknown.value?.id))
      unknown.value = undefined;
    if (
      settingUnknown.value &&
      result.value.settings?.operations.some((r) => r.id === settingUnknown.value?.id)
    )
      settingUnknown.value = undefined;
  }
  function failed(value: unknown) {
    const issue = failure(value);
    error.value = issue.message;
    if (['session_expired', 'profile_changed', 'stale'].includes(issue.code)) {
      epoch = undefined;
      serial++;
      page.value = undefined;
      unknown.value = undefined;
      settingUnknown.value = undefined;
      busy.value = false;
      loading.value = false;
      expired();
      return true;
    }
    return false;
  }
  async function read(refresh: boolean) {
    if (!epoch || disposed || busy.value) return;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && epoch === owner && current === serial;
    loading.value = true;
    error.value = '';
    try {
      const result = await core.relayView(owner, refresh);
      if (active()) accept(result, owner);
    } catch (e) {
      if (active()) failed(e);
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function command(command: RelayCommand | RelaySettingCommand) {
    if (!epoch || disposed || busy.value || loading.value) return false;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && epoch === owner && current === serial;
    busy.value = true;
    error.value = '';
    try {
      const result = isRelaySettingCommand(command)
        ? await core.relaySettingCommand(owner, command)
        : await core.relayCommand(owner, command);
      if (!active()) return false;
      accept(result, owner);
      return true;
    } catch (e) {
      if (!active() || failed(e)) return false;
      if (command.kind === 'setting_change') settingUnknown.value = command;
      if (command.kind === 'continue' || command.kind === 'retry') {
        unknown.value = {
          id: command.id,
          revision: command.kind === 'continue' ? command.revision : (page.value?.revision ?? ''),
        };
      }
      if (
        command.kind === 'continue' ||
        command.kind === 'retry' ||
        isRelaySettingCommand(command)
      ) {
        try {
          const result = await core.relayView(owner, false);
          if (active()) accept(result, owner);
        } catch {
          /* Keep unknown; never replay. */
        }
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  async function continueHere(quality: Quality = 'standard') {
    if (!page.value?.alert || unknown.value || busy.value || loading.value || error.value)
      return false;
    hidden.value = page.value.revision;
    return command({
      kind: 'continue',
      id: crypto.randomUUID(),
      revision: page.value.revision,
      quality,
    });
  }
  async function retry(previous_id: string, quality: Quality = 'standard') {
    if (unknown.value || !page.value?.operations.some((r) => r.id === previous_id && r.canRetry))
      return false;
    return command({ kind: 'retry', id: crypto.randomUUID(), previous_id, quality });
  }
  async function dismiss() {
    if (!page.value?.alert) return;
    hidden.value = page.value.revision;
    await command({ kind: 'dismiss', revision: page.value.revision });
  }
  async function cancelUnknown() {
    if (unknown.value) await command({ kind: 'cancel_unseen', ...unknown.value });
  }
  async function changeSetting(enabled: boolean) {
    const settings = page.value?.settings;
    if (!canChangeSetting.value || !settings || settings.enabled === enabled) return false;
    return command({
      kind: 'setting_change',
      id: crypto.randomUUID(),
      revision: settings.revision,
      enabled,
    });
  }
  async function reconcileSetting(id: string) {
    if (!page.value?.settings?.operations.some((op) => op.id === id && relaySettingPending(op)))
      return false;
    return command({ kind: 'setting_reconcile', id });
  }
  async function acceptSetting(id: string, revision: string) {
    if (
      !page.value?.settings?.operations.some(
        (op) => op.id === id && relaySettingPending(op) && op.readRevision === revision,
      )
    )
      return false;
    return command({ kind: 'setting_accept', id, revision });
  }
  async function cancelUnknownSetting() {
    if (settingUnknown.value)
      return command({ ...settingUnknown.value, kind: 'setting_cancel_unseen' });
    return false;
  }
  function dispose() {
    disposed = true;
    serial++;
  }
  return {
    page,
    alert,
    loading,
    busy,
    error,
    unknown,
    settingUnknown,
    canChangeSetting,
    changeSetting,
    reconcileSetting,
    acceptSetting,
    cancelUnknownSetting,
    connect,
    read,
    continueHere,
    retry,
    dismiss,
    cancelUnknown,
    dispose,
  };
}
export type RelayModel = ReturnType<typeof useRelay>;
