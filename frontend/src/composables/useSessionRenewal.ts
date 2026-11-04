import { ref, shallowRef } from 'vue';
import type { RenewalClient, SessionRenewalView } from '../services/renewal.ts';
import { failure } from '../services/contracts.ts';

export function useSessionRenewal(core: RenewalClient, epoch: string) {
  const view = shallowRef<SessionRenewalView>();
  const busy = ref(false),
    error = ref('');
  let alive = true,
    sequence = 0,
    stop: (() => void) | undefined;
  const current = (serial: number) => alive && serial === sequence;
  const accept = (next: SessionRenewalView) => {
    if (alive && next.epoch === epoch) view.value = next;
  };
  async function read() {
    if (!alive) return;
    const serial = ++sequence;
    try {
      const next = await core.sessionRenewal(epoch);
      if (current(serial)) {
        accept(next);
        error.value = '';
      }
    } catch (e) {
      if (current(serial)) error.value = failure(e).message;
    }
  }
  async function load() {
    if (!busy.value) await read();
  }
  async function connect() {
    try {
      const off = await core.onSessionRenewal((next) => {
        if (alive && next.epoch === epoch) {
          sequence++;
          accept(next);
        }
      });
      if (alive) stop = off;
      else off();
    } catch {
      if (alive) error.value = '续期状态订阅失败，可以手动检查';
    }
    await load();
  }
  async function run() {
    if (!alive || busy.value || view.value?.busy) return;
    const action = view.value?.canReconcile
      ? 'reconcile'
      : view.value?.canCheck
        ? 'check'
        : undefined;
    if (!action) return;
    busy.value = true;
    error.value = '';
    let prior = '';
    const serial = ++sequence;
    try {
      const next = await core.renewSession(epoch, action);
      if (current(serial)) accept(next);
    } catch (e) {
      prior = failure(e).message;
    } finally {
      // Lost IPC replies only read durable state. Never replay a renewal.
      await read();
      if (alive) {
        busy.value = false;
        if (prior && !error.value) {
          const state = view.value?.state;
          // A periodic error may carry a generic 301 translation even when
          // the native account is still active. The durable view wins.
          if (state === 'recovery_required') error.value = '暂时无法核对账号身份，请稍后再试';
          else if (state !== 'unconfirmed' && state !== 'expired') error.value = prior;
        }
      }
    }
  }
  function dispose() {
    alive = false;
    sequence++;
    stop?.();
  }
  return { view, busy, error, connect, load, run, dispose };
}
