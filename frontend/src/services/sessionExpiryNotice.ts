import type { AccountView } from './contracts';

interface Clock {
  schedule(callback: () => void, delay: number): () => void;
}

const localClock: Clock = {
  schedule(callback, delay) {
    const timer = setTimeout(callback, delay);
    return () => clearTimeout(timer);
  },
};

export function liveSessionExpiryNotice(account: AccountView) {
  const notice = account.loginNotice;
  return account.status === 'expired' && account.liveJskExpiry && notice?.kind === 'expired'
    ? notice
    : undefined;
}

export function createSessionExpiryNotice(show: (message?: string) => void, clock = localClock) {
  let active: { epoch: string; cancel: () => void } | undefined;
  let considered: string | undefined;

  function clear() {
    const previous = active;
    if (!previous) return;
    active = undefined;
    previous.cancel();
    show();
  }

  function observe(next: AccountView, previous: AccountView) {
    const notice = liveSessionExpiryNotice(next);
    if (active && (active.epoch !== next.epoch || !notice)) clear();
    if (
      previous.status !== 'authenticated' ||
      previous.epoch === next.epoch ||
      !notice ||
      considered === next.epoch
    )
      return;

    considered = next.epoch;
    const expiry = {
      epoch: next.epoch,
      cancel: clock.schedule(() => {
        if (active === expiry) clear();
      }, 7000),
    };
    active = expiry;
    show(notice.message);
  }

  return { observe, dispose: clear };
}
