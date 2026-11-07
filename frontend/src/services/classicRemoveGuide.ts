import type { AccountView } from './contracts';

export interface ClassicRemoveGuide {
  epoch: string;
  ticket: string;
  title: string | null;
  message: string;
}

interface Clock {
  schedule(callback: () => void, delay: number): () => void;
}

const localClock: Clock = {
  schedule(callback, delay) {
    const timer = setTimeout(callback, delay);
    return () => clearTimeout(timer);
  },
};

export function createClassicRemoveGuideScheduler(
  canOpen: () => boolean,
  open: (guide: ClassicRemoveGuide) => void,
  dismiss: (guide: ClassicRemoveGuide) => void,
  clock = localClock,
) {
  let considered: { epoch: string; ticket: string } | undefined;
  let pending: ClassicRemoveGuide | undefined;
  let cancel: (() => void) | undefined;

  function clear() {
    cancel?.();
    cancel = undefined;
    const old = pending;
    pending = undefined;
    if (old) dismiss(old);
  }

  function observe(account: AccountView) {
    if (considered && considered.epoch !== account.epoch) considered = undefined;
    const ticket = account.classicRemoveGuide;
    const notice = account.loginNotice;
    if (account.status !== 'expired' || notice?.kind !== 'reauthenticate' || !ticket) {
      clear();
      return;
    }
    if (considered?.ticket === ticket) return;
    clear();
    considered = { epoch: account.epoch, ticket };
    const guide = { ...considered, title: notice.title, message: notice.message };
    pending = guide;
    cancel = clock.schedule(() => {
      if (pending !== guide) return;
      pending = undefined;
      cancel = undefined;
      if (canOpen()) open(guide);
      else dismiss(guide);
    }, 1000);
  }

  return { observe, dispose: clear };
}
