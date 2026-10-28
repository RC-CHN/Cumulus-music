import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import type {
  StartupPreferencesClient,
  StartupPreferencesCommand,
  StartupPreferencesView,
  StartupSettings,
} from '../services/startup.ts';
import { useCore } from '../services/core.ts';

export function useStartupPreferences(core: StartupPreferencesClient = useCore()) {
  const view = shallowRef<StartupPreferencesView>();
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    saved = ref(false);
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  const canChange = computed(
    () =>
      !!view.value?.revision &&
      !view.value.damaged &&
      !loading.value &&
      !busy.value &&
      !error.value,
  );
  function accept(result: AccountData<StartupPreferencesView>, owner: string) {
    if (result.epoch !== owner) throw { code: 'stale', message: '账号已变化，请重新读取播放设置' };
    view.value = result.value;
  }
  async function read() {
    if (disposed || !epoch || busy.value) return;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && owner === epoch && current === serial;
    loading.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.startupPreferences(owner);
      if (active()) accept(result, owner);
    } catch (e) {
      if (active()) error.value = failure(e).message;
    } finally {
      if (active()) loading.value = false;
    }
  }
  function connect(account: AccountView) {
    if (account.epoch === epoch) return;
    epoch = account.epoch;
    serial++;
    view.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    saved.value = false;
    return read();
  }
  async function command(command: StartupPreferencesCommand) {
    if (disposed || !epoch || busy.value || loading.value) return false;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && owner === epoch && current === serial;
    busy.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.startupPreferencesCommand(owner, command);
      if (!active()) return false;
      accept(result, owner);
      saved.value = true;
      return true;
    } catch (e) {
      if (active()) {
        error.value = failure(e).message;
        // Local CAS writes are atomic; read current state after a lost reply.
        // Never resend a change or label readback as acknowledgement of ours.
        try {
          const result = await core.startupPreferences(owner);
          if (active()) accept(result, owner);
        } catch {
          /* Keep the error and require explicit refresh. */
        }
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  function change(key: keyof StartupSettings, value: boolean) {
    if (!canChange.value || !view.value?.revision || view.value.settings[key] === value)
      return Promise.resolve(false);
    return command({
      kind: 'change',
      expected_revision: view.value.revision,
      settings: { ...view.value.settings, [key]: value },
    });
  }
  function repair() {
    return view.value?.damaged ? command({ kind: 'repair' }) : Promise.resolve(false);
  }
  function dispose() {
    disposed = true;
    serial++;
  }
  return { view, loading, busy, error, saved, canChange, connect, read, change, repair, dispose };
}
