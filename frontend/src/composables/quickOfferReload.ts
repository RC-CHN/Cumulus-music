import { shallowRef, watch } from 'vue';

const update = shallowRef<{ epoch: string }>();
let pendingEpoch: string | undefined;

// Settings may stay mounted behind the chooser, where a native eligibility
// check can revoke the current account's local offer.
export function queueQuickOfferReload(epoch: string) {
  pendingEpoch = epoch;
}

export function flushQuickOfferReload() {
  const epoch = pendingEpoch;
  pendingEpoch = undefined;
  if (epoch) update.value = { epoch };
}

export function onQuickOfferReload(epoch: string, reload: () => void) {
  watch(update, (next) => {
    if (next?.epoch === epoch) reload();
  });
}
