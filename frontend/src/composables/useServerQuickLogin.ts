import { computed, ref, shallowRef } from 'vue';
import type {
  ServerQuickLoginClient,
  ServerQuickLoginOperationView,
} from '../services/serverQuickLogin.ts';

interface Operation {
  epoch: string;
  id: string;
}

export function useServerQuickLogin(
  client: ServerQuickLoginClient,
  guestEpoch: () => string | null,
  authorized: () => void,
) {
  const view = shallowRef<ServerQuickLoginOperationView>();
  const pending = ref<'preparing' | 'exchanging' | 'reconciling'>();
  const cancelling = ref(false);
  const problem = ref<'preflight' | 'read_required'>();
  let operation: Operation | undefined;
  let serial = 0;
  let disposed = false;
  let published = false;

  const canRead = computed(
    () =>
      !!view.value &&
      (problem.value === 'read_required' ||
        view.value.canReconcile ||
        view.value.phase === 'exchanging' ||
        view.value.phase === 'checking_identity'),
  );
  function clear() {
    serial++;
    operation = undefined;
    view.value = undefined;
    pending.value = undefined;
    cancelling.value = false;
    problem.value = undefined;
  }
  function bestEffortCancel(target: Operation) {
    void client.serverQuickLoginCommand(target.epoch, target.id, 'cancel').catch(() => {});
  }
  function active(request: number, epoch: string) {
    return !disposed && request === serial && guestEpoch() === epoch;
  }
  function accept(target: Operation, value: ServerQuickLoginOperationView) {
    if (value.operationId !== target.id) throw Error('operation mismatch');
    view.value = value;
    problem.value = undefined;
    if (value.phase === 'authenticated' && !published) {
      published = true;
      authorized();
    }
  }
  async function start(savedId: string, consent: boolean) {
    const epoch = guestEpoch();
    if (!consent || !epoch || disposed || pending.value || operation || problem.value) return;
    const request = ++serial;
    pending.value = 'preparing';
    try {
      const response = await client.beginServerQuickLogin(epoch, savedId, consent);
      const target = { epoch, id: response.value.operationId };
      if (!active(request, epoch)) {
        if (response.epoch === epoch) bestEffortCancel(target);
        return;
      }
      if (response.epoch !== epoch || response.value.phase !== 'ready') {
        if (response.epoch === epoch) bestEffortCancel(target);
        throw Error('quick preflight mismatch');
      }
      operation = target;
      accept(target, response.value);
      pending.value = 'exchanging';
      const result = await client.serverQuickLoginCommand(epoch, target.id, 'execute');
      if (!active(request, epoch)) return;
      if (result.epoch !== epoch) throw Error('account mismatch');
      accept(target, result.value);
    } catch {
      if (active(request, epoch)) problem.value = operation ? 'read_required' : 'preflight';
    } finally {
      if (active(request, epoch)) pending.value = undefined;
    }
  }
  async function reconcile() {
    const target = operation;
    if (!target || !canRead.value || pending.value || cancelling.value) return;
    const request = serial;
    pending.value = 'reconciling';
    try {
      const result = await client.serverQuickLoginCommand(target.epoch, target.id, 'reconcile');
      if (!active(request, target.epoch)) return;
      if (result.epoch !== target.epoch) throw Error('account mismatch');
      accept(target, result.value);
    } catch {
      if (active(request, target.epoch)) problem.value = 'read_required';
    } finally {
      if (active(request, target.epoch)) pending.value = undefined;
    }
  }
  async function cancel() {
    if (cancelling.value || published) return false;
    const target = operation;
    if (!target || view.value?.phase === 'cancelled') {
      // The native begin is only preparation. A late Ready is cancelled
      // by start(), and never advances to the exchange.
      clear();
      return true;
    }
    const request = serial;
    cancelling.value = true;
    try {
      const result = await client.serverQuickLoginCommand(target.epoch, target.id, 'cancel');
      if (!active(request, target.epoch)) return false;
      if (result.epoch !== target.epoch) throw Error('account mismatch');
      accept(target, result.value);
      if (result.value.phase !== 'cancelled') return false;
      clear();
      return true;
    } catch {
      if (active(request, target.epoch)) problem.value = 'read_required';
      return false;
    } finally {
      if (active(request, target.epoch)) cancelling.value = false;
    }
  }
  function dispose() {
    if (disposed) return;
    disposed = true;
    if (operation && !published && !cancelling.value) bestEffortCancel(operation);
    clear();
  }
  return { view, pending, cancelling, problem, canRead, start, reconcile, cancel, dispose };
}
