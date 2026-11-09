import { computed, onScopeDispose, ref, watch, type Ref } from 'vue';
import { failure, type CoreFailure } from '../services/contracts.ts';

export interface VolumeControls {
  view: Readonly<Ref<{ epoch: string; volume: number; unmuteVolume?: number | null } | undefined>>;
  ready: Readonly<Ref<boolean>>;
  scope?: Readonly<Ref<string | undefined>>;
  configureVolume(value: number): Promise<unknown>;
}

export function usePlaybackVolume(player: VolumeControls) {
  const pending = ref(false),
    dragging = ref(false),
    draft = ref<number>(),
    issue = ref<CoreFailure>();
  const native = computed(() => player.view.value?.volume ?? 100);
  const value = computed(() => draft.value ?? native.value);
  const percent = computed(() =>
    value.value <= 0
      ? 0
      : value.value >= 100
        ? 100
        : Math.min(99, Math.max(1, Math.round(value.value))),
  );
  let inputRevision = 0,
    settlement = Promise.resolve(),
    generation = 0,
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
  watch([() => player.view.value?.epoch, player.ready, () => player.scope?.value], reset, {
    flush: 'sync',
  });
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
    inputRevision++;
    const target = Math.max(0, Math.min(100, input));
    if (target > 0) restore = target;
    draft.value = target;
    issue.value = undefined;
    retryTarget = undefined;
    requested = target;
    if (!pending.value) settlement = drain();
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
  return {
    value,
    native,
    percent,
    inputRevision: () => inputRevision,
    settled: () => settlement,
    pending,
    dragging,
    issue,
    canRetry,
    set,
    toggleMute,
    wheel,
    begin,
    end,
    retry,
  };
}
