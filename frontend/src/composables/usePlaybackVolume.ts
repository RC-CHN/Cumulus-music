import { computed, onScopeDispose, ref, watch } from 'vue';
import type { PlayerModel } from './usePlayer';
import { failure, type CoreFailure } from '../services/contracts.ts';

export function usePlaybackVolume(player: PlayerModel) {
  const pending = ref(false),
    dragging = ref(false),
    draft = ref<number>(),
    issue = ref<CoreFailure>();
  const native = computed(() => player.view.value?.volume ?? 100);
  const value = computed(() => draft.value ?? native.value);
  let generation = 0,
    alive = true,
    requested: number | undefined,
    retryTarget: number | undefined,
    restore = player.view.value?.unmuteVolume ?? 10;
  function reset() {
    generation++;
    requested = retryTarget = undefined;
    pending.value = dragging.value = false;
    draft.value = issue.value = undefined;
    restore = player.view.value?.unmuteVolume ?? 10;
  }
  watch([() => player.view.value?.epoch, player.ready], reset, { flush: 'sync' });
  watch(
    () => player.view.value?.unmuteVolume,
    (saved) => {
      if (!pending.value && saved != null) restore = saved;
    },
    { flush: 'sync' },
  );
  async function drain() {
    const current = generation;
    pending.value = true;
    try {
      while (requested !== undefined && alive && current === generation) {
        const target = requested;
        requested = undefined;
        try {
          if (target !== native.value) await player.configureVolume(target);
        } catch (error) {
          if (alive && current === generation) {
            issue.value = failure(error);
            retryTarget = requested ?? target;
            requested = undefined;
            draft.value = undefined;
          }
          break;
        }
      }
    } finally {
      if (alive && current === generation) {
        pending.value = false;
        if (!dragging.value || issue.value) draft.value = undefined;
        restore = player.view.value?.unmuteVolume ?? 10;
      }
    }
  }
  function set(input: number) {
    if (!alive || !player.ready.value || !Number.isFinite(input)) return;
    const target = Math.max(0, Math.min(100, Math.round(input)));
    if (target > 0) restore = target;
    draft.value = target;
    issue.value = undefined;
    retryTarget = undefined;
    requested = target;
    if (!pending.value) void drain();
  }
  const toggleMute = () => set(value.value > 0 ? 0 : restore);
  const wheel = (delta: number) => {
    if (delta !== 0 && Number.isFinite(delta)) set(value.value + (delta > 0 ? -3 : 3));
  };
  function begin() {
    dragging.value = true;
    draft.value = value.value;
  }
  function end() {
    dragging.value = false;
    if (!pending.value) draft.value = undefined;
  }
  const canRetry = computed(
    () => player.ready.value && retryTarget !== undefined && issue.value?.retryable !== false,
  );
  function retry() {
    if (canRetry.value) set(retryTarget!);
  }
  onScopeDispose(() => {
    alive = false;
    reset();
  });
  return { value, pending, dragging, issue, canRetry, set, toggleMute, wheel, begin, end, retry };
}
