import { computed, ref, shallowRef } from 'vue';
import type { AuthClient, LoginMode, LoginCommand, LoginView } from '../services/auth.ts';
import { failure } from '../services/contracts.ts';

export function useLogin(core: AuthClient, authorized: () => void = () => {}) {
  const view = shallowRef<LoginView>();
  const busy = ref(false);
  const error = ref('');
  const remaining = ref(0);
  const seconds = computed(() => Math.ceil(remaining.value / 1000));
  let generation = 0;
  let disposed = false;
  let deadline = 0;
  let timer: ReturnType<typeof setInterval> | undefined;
  function accept(next: LoginView) {
    view.value = next;
    deadline = Date.now() + next.retryAfterMs;
    remaining.value = next.retryAfterMs;
    clearInterval(timer);
    if (next.retryAfterMs)
      timer = setInterval(() => {
        remaining.value = Math.max(0, deadline - Date.now());
        if (!remaining.value) clearInterval(timer);
      }, 250);
  }
  async function cancel(id: string) {
    try {
      await core.cancelLogin(id);
    } catch {
      /* Already superseded/terminal. */
    }
  }
  async function start(epoch: string, mode: LoginMode, savedId?: string) {
    if (disposed) return;
    const serial = ++generation;
    const old = view.value;
    view.value = undefined;
    error.value = '';
    busy.value = true;
    if (old) await cancel(old.operationId);
    if (disposed || serial !== generation) return;
    try {
      const next = await core.beginLogin(epoch, mode, savedId);
      if (disposed || serial !== generation) {
        await cancel(next.operationId);
        return;
      }
      accept(next);
      return next;
    } catch (e) {
      if (!disposed && serial === generation) error.value = failure(e).message;
    } finally {
      if (!disposed && serial === generation) busy.value = false;
    }
  }
  async function execute(call: (id: string) => Promise<LoginView>) {
    const current = view.value;
    if (disposed || busy.value || !current) return;
    const serial = generation;
    busy.value = true;
    error.value = '';
    try {
      const next = await call(current.operationId);
      if (disposed || serial !== generation) return;
      accept(next);
      if (next.status === 'authenticated') authorized();
      return next;
    } catch (e) {
      if (disposed || serial !== generation) return;
      error.value = failure(e).message;
      // A failed write can have committed credentials. Read only; never replay
      // passwords, SMS/email sends or verification answers automatically.
      try {
        const next = await core.loginSnapshot(current.operationId);
        if (!disposed && serial === generation) {
          accept(next);
          if (next.status === 'authenticated') authorized();
        }
      } catch {
        /* Keep the original bounded error. */
      }
    } finally {
      if (!disposed && serial === generation) busy.value = false;
    }
  }
  function run(command: LoginCommand) {
    return execute((id) => core.loginCommand(id, command));
  }
  function captcha() {
    return execute(async (id) => {
      const next = await core.solveLoginCaptcha(id);
      if (next.kind !== 'credentials') throw new Error('Unexpected login state');
      return next.value;
    });
  }
  function dispose() {
    disposed = true;
    generation++;
    clearInterval(timer);
    const id = view.value?.operationId;
    view.value = undefined;
    if (id) void cancel(id);
  }
  return { view, busy, error, seconds, start, run, captcha, dispose };
}
export type LoginModel = ReturnType<typeof useLogin>;
