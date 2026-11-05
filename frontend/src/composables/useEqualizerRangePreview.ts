import { computed, shallowRef, watch } from 'vue';

type RangeTarget = Pick<HTMLInputElement, 'value' | 'valueAsNumber'>;

/** Pointer movement is local UI feedback. Only a completed range change
 * reaches the durable equalizer draft. */
export function useEqualizerRangePreview<Key>(
  scope: () => string | undefined,
  committed: (key: Key) => number,
  apply: (key: Key, value: number) => void,
) {
  const active = shallowRef<{ key: Key; target: RangeTarget; value: number }>();
  const cancelled = new WeakSet<RangeTarget>();

  function cancel(target = active.value?.target) {
    if (!target) return;
    cancelled.add(target);
    const preview = active.value;
    if (!preview || preview.target !== target) return;
    active.value = undefined;
    preview.target.value = String(committed(preview.key));
  }

  watch(scope, () => cancel(), { flush: 'sync' });

  function begin(target: RangeTarget) {
    if (active.value?.target !== target) cancel();
    if (scope()) cancelled.delete(target);
  }

  function preview(key: Key, target: RangeTarget) {
    if (!scope() || cancelled.has(target)) {
      target.value = String(committed(key));
      return;
    }
    if (!Number.isFinite(target.valueAsNumber)) return;
    if (active.value?.target !== target) cancel();
    active.value = { key, target, value: target.valueAsNumber };
  }

  function commit(key: Key, target: RangeTarget) {
    if (!scope() || cancelled.has(target)) {
      cancel(target);
      target.value = String(committed(key));
      return;
    }
    if (active.value?.target === target) active.value = undefined;
    else cancel();
    const value = target.valueAsNumber;
    if (Number.isFinite(value)) apply(key, value);
  }

  return {
    previewing: computed(() => active.value !== undefined),
    value: (key: Key) => {
      const preview = active.value;
      return preview && preview.key === key ? preview.value : committed(key);
    },
    begin,
    preview,
    commit,
    cancel,
  };
}
