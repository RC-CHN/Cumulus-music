import type { LoginStatus, SecurityQrVerificationView } from './auth';
import type { QrStatus } from './contracts';

export function securityQrActions(
  qr: Pick<SecurityQrVerificationView, 'status' | 'canRefresh' | 'canReconcile'> | null | undefined,
  loginStatus: LoginStatus | undefined,
  pollingFailed = false,
) {
  if (!qr) return { canRefresh: false, canReconcile: false };
  const canReconcile = qr.canReconcile ?? (qr.status === 'authorized' && loginStatus === 'error');
  const canRefresh =
    !canReconcile &&
    (qr.canRefresh ??
      (qr.status !== 'authorized' &&
        (pollingFailed ||
          loginStatus === 'error' ||
          ['idle', 'expired', 'error', 'cancelled', 'challenge'].includes(qr.status))));
  return { canRefresh, canReconcile };
}

export interface SecurityQrPollState {
  owner: string;
  verificationId: string;
  status: QrStatus;
  loginStatus: LoginStatus;
  pollAfterMs: number;
  busy: boolean;
  active: boolean;
  canReconcile?: boolean;
}
export interface SecurityQrClock {
  schedule(callback: () => void, delay: number): () => void;
}
const timeoutClock: SecurityQrClock = {
  schedule(callback, delay) {
    const timer = setTimeout(callback, delay);
    return () => clearTimeout(timer);
  },
};

export function createSecurityQrPoller(options: {
  read: () => SecurityQrPollState | null;
  poll: (verificationId: string) => Promise<boolean>;
  failed: () => void;
  initialRefresh?: () => void;
  clock?: SecurityQrClock;
}) {
  const clock = options.clock ?? timeoutClock;
  let cancelTimer: (() => void) | undefined;
  let deadline: object | undefined;
  let owner: string | undefined;
  let generation = 0;
  let inFlight = false;
  let halted = false;
  let disposed = false;
  let initialChecked = false;
  function stopTimer() {
    cancelTimer?.();
    cancelTimer = undefined;
    deadline = undefined;
  }
  function eligible(state: SecurityQrPollState | null): state is SecurityQrPollState {
    return (
      !!state &&
      state.active &&
      !state.busy &&
      !state.canReconcile &&
      state.loginStatus === 'security_qr' &&
      (state.status === 'waiting_scan' || state.status === 'waiting_confirmation') &&
      Number.isFinite(state.pollAfterMs) &&
      state.pollAfterMs > 0
    );
  }
  function sync() {
    stopTimer();
    if (disposed) return;
    const state = options.read();
    if (state?.owner !== owner) {
      owner = state?.owner;
      generation++;
      halted = false;
    }
    if (!initialChecked && state?.active && !state.busy) {
      initialChecked = true;
      if (state.status === 'idle' && !state.canReconcile && options.initialRefresh) {
        options.initialRefresh();
        return;
      }
    }
    if (halted || inFlight || !eligible(state)) return;
    const serial = generation;
    const token = {};
    deadline = token;
    cancelTimer = clock.schedule(
      () => {
        if (deadline !== token) return;
        cancelTimer = undefined;
        deadline = undefined;
        void poll(state.owner, serial);
      },
      Math.max(100, Math.min(state.pollAfterMs, 60000)),
    );
  }
  async function poll(expectedOwner: string, serial: number) {
    const state = options.read();
    if (
      disposed ||
      halted ||
      inFlight ||
      serial !== generation ||
      state?.owner !== expectedOwner ||
      !eligible(state)
    )
      return;
    inFlight = true;
    let success = false;
    try {
      success = await options.poll(state.verificationId);
    } catch {
      // The caller retains bounded native errors; no request is replayed here.
    } finally {
      inFlight = false;
      if (!disposed && serial === generation && options.read()?.owner === expectedOwner) {
        if (!success) {
          halted = true;
          options.failed();
        }
      }
      sync();
    }
  }
  function pause() {
    generation++;
    halted = true;
    stopTimer();
  }
  function resume() {
    halted = false;
    sync();
  }
  function dispose() {
    disposed = true;
    pause();
  }
  return { sync, pause, resume, dispose };
}
