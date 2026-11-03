import { ref, shallowRef } from 'vue';
import type {
  QuickLoginEligibilityClient,
  QuickLoginEligibilityView,
} from '../services/quickLoginEligibility.ts';

export function useQuickLoginEligibility(client: QuickLoginEligibilityClient, owner: () => string) {
  const view = shallowRef<QuickLoginEligibilityView>();
  const state = ref<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const busy = ref(false);
  let serial = 0;
  let disposed = false;

  function reset() {
    serial++;
    view.value = undefined;
    state.value = 'idle';
  }
  function dispose() {
    disposed = true;
    reset();
  }
  async function check() {
    if (disposed || busy.value) return;
    const epoch = owner();
    const request = ++serial;
    const active = () => !disposed && request === serial && epoch === owner();
    view.value = undefined;
    state.value = 'loading';
    busy.value = true;
    try {
      const response = await client.quickLoginEligibility(epoch);
      if (!active()) return;
      if (response.epoch !== epoch) {
        state.value = 'error';
        return;
      }
      view.value = response.value;
      state.value = 'ready';
    } catch {
      if (active()) state.value = 'error';
    } finally {
      if (!disposed) {
        busy.value = false;
        if (state.value === 'loading') state.value = 'idle';
      }
    }
  }
  return { view, state, busy, check, reset, dispose };
}
