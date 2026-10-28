import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import type { OutputClient, OutputCommand, OutputView } from '../services/output.ts';
import { useCore } from '../services/core.ts';
export function useOutputSettings(core: OutputClient = useCore()) {
  const view = shallowRef<OutputView>();
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    saved = ref(false);
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const canChange = computed(
    () =>
      view.value?.revision != null &&
      !view.value.damaged &&
      view.value.supported !== false &&
      !loading.value &&
      !busy.value &&
      !error.value,
  );
  function stopTimer() {
    clearTimeout(timer);
    timer = undefined;
  }
  function schedule() {
    stopTimer();
    if (!disposed && !error.value) timer = setTimeout(() => void read(false, true), 2000);
  }
  function accept(result: AccountData<OutputView>, owner: string) {
    if (result.epoch !== owner) throw { code: 'stale', message: '账号已变化，请重新读取输出设备' };
    view.value = result.value;
  }
  async function read(refresh = true, quiet = false) {
    if (disposed || !epoch || busy.value || loading.value) return;
    stopTimer();
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && owner === epoch && current === serial;
    loading.value = !quiet;
    if (!quiet) {
      error.value = '';
      saved.value = false;
    }
    try {
      const result = await core.outputSettings(owner, refresh);
      if (active()) accept(result, owner);
    } catch (e) {
      if (active()) error.value = failure(e).message;
    } finally {
      if (active()) {
        loading.value = false;
        schedule();
      }
    }
  }
  function connect(account: AccountView) {
    if (account.epoch === epoch) return;
    stopTimer();
    epoch = account.epoch;
    serial++;
    view.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    saved.value = false;
    return read();
  }
  async function command(command: OutputCommand) {
    if (disposed || !epoch || busy.value || loading.value) return false;
    stopTimer();
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && owner === epoch && current === serial;
    busy.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.outputCommand(owner, command);
      if (!active()) return false;
      accept(result, owner);
      saved.value = true;
      return true;
    } catch (e) {
      if (active()) {
        error.value = failure(e).message;
        // A lost reply is not permission to repeat a local write.
        try {
          const result = await core.outputSettings(owner, false);
          if (active()) accept(result, owner);
        } catch {
          /* Explicit refresh remains available. */
        }
      }
      return false;
    } finally {
      if (active()) {
        busy.value = false;
        schedule();
      }
    }
  }
  function select(id: string) {
    if (!canChange.value || view.value?.revision == null || id === view.value.selected.id)
      return Promise.resolve(false);
    return command({ kind: 'select', expected_revision: view.value.revision, id });
  }
  function retry() {
    return canChange.value && view.value?.revision != null
      ? command({ kind: 'retry', expected_revision: view.value.revision })
      : Promise.resolve(false);
  }
  function repair() {
    return view.value?.damaged ? command({ kind: 'repair' }) : Promise.resolve(false);
  }
  function dispose() {
    disposed = true;
    serial++;
    stopTimer();
  }
  return {
    view,
    loading,
    busy,
    error,
    saved,
    canChange,
    connect,
    read,
    select,
    retry,
    repair,
    dispose,
  };
}
