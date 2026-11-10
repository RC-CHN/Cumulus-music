import { computed, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { EffectDirectoryClient } from '../services/effectDirectory.ts';
import type { PlayerView } from '../services/player.ts';

type Attempt = { key: string; receipt: string };

export function useHeadphoneHeader(
  core: Pick<EffectDirectoryClient, 'effectDeviceCompare'>,
  account: () => AccountView,
  player: () => PlayerView | undefined,
  modern: () => boolean,
) {
  const pending = shallowRef<Attempt>();
  const error = shallowRef('');
  const previousEnabled = shallowRef<string>();
  let disposed = false;
  const target = computed(() => {
    const owner = account();
    const view = player();
    const device = view?.deviceParametric;
    const comparison = device?.comparison;
    if (
      owner.status !== 'authenticated' ||
      !view ||
      view.epoch !== owner.epoch ||
      !device ||
      !comparison ||
      comparison.playId !== view.playId ||
      (device.status !== 'configured' && device.status !== 'paused_locally')
    )
      return;
    return {
      epoch: owner.epoch,
      key: `${owner.epoch}:${device.activationId}:${comparison.playId}`,
      device,
      comparison,
    };
  });
  const visible = computed(() => {
    const state = target.value;
    return (
      !!state &&
      (state.device.status === 'configured' || !modern() || previousEnabled.value === state.key)
    );
  });
  const stop = watch(
    [
      () => target.value?.key,
      () => target.value?.comparison.receipt,
      () => target.value?.device.status,
    ],
    ([key, receipt, status], [previousKey]) => {
      if (key !== previousKey) previousEnabled.value = undefined;
      if (key && status === 'configured') previousEnabled.value = key;
      if (pending.value && (pending.value.key !== key || pending.value.receipt !== receipt))
        pending.value = undefined;
      error.value = '';
    },
    { immediate: true, flush: 'sync' },
  );

  async function toggle(displayedReceipt?: string) {
    const state = target.value;
    if (
      disposed ||
      !visible.value ||
      !state ||
      pending.value ||
      displayedReceipt !== state.comparison.receipt
    )
      return;
    const attempt = { key: state.key, receipt: displayedReceipt };
    pending.value = attempt;
    error.value = '';
    try {
      await core.effectDeviceCompare(state.epoch, {
        ...state.comparison,
        brand: state.device.brand,
        model: state.device.model,
        activationId: state.device.activationId,
        paused: state.device.status === 'configured',
      });
      // The command returns no status. Only the next native receipt changes
      // the display and unlocks another click on this activation.
    } catch (issue) {
      if (!disposed && pending.value === attempt) {
        pending.value = undefined;
        error.value = failure(issue).message;
      }
    }
  }

  function dispose() {
    disposed = true;
    stop();
    pending.value = undefined;
    error.value = '';
  }
  return { visible, target, pending, error, toggle, dispose };
}
