import { shallowRef, watch } from 'vue';

// A native mutation can settle after the dialog that started it has gone away.
// Keep the invalidation scoped to the same account and re-read only local state.
const changed = shallowRef<{ epoch: string }>();

export function effectRegistrationChanged(epoch: string) {
  changed.value = { epoch };
}

export function useEffectRegistrationChanges(epoch: () => string, refresh: () => void) {
  watch(
    changed,
    (change) => {
      if (change?.epoch === epoch()) refresh();
    },
    { flush: 'sync' },
  );
}
