import { computed, onScopeDispose, ref, watch } from 'vue';
import type { PlayerModel } from './usePlayer';
import type { IntelligenceOrigin } from '../services/player.ts';
import { failure, type CoreFailure, type CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';

export function useIntelligence(
  player: PlayerModel,
  together: () => boolean,
  core: CoreClient = useCore(),
) {
  const busy = ref(false),
    cancelling = ref(false),
    issue = ref<CoreFailure>();
  let pending: { id: string; epoch: string } | undefined,
    generation = 0,
    alive = true;
  const active = computed(() =>
    ['seeded', 'active'].includes(player.view.value?.intelligence ?? ''),
  );
  const resumable = computed(
    () =>
      player.ready.value &&
      player.view.value?.intelligence === 'seeded' &&
      !busy.value &&
      !together(),
  );
  const canStart = computed(
    () => player.ready.value && !busy.value && !active.value && !together(),
  );
  async function cancel() {
    const operation = pending;
    if (!operation || cancelling.value) return;
    cancelling.value = true;
    try {
      await core.playerIntelligenceCancel(operation.epoch, operation.id);
    } catch (e) {
      if (pending === operation && alive) {
        issue.value = failure(e);
        cancelling.value = false;
      }
    }
  }
  function reset() {
    const operation = pending;
    generation++;
    pending = undefined;
    busy.value = cancelling.value = false;
    issue.value = undefined;
    if (operation)
      void core.playerIntelligenceCancel(operation.epoch, operation.id).catch(() => {});
  }
  watch([() => player.view.value?.epoch, player.ready], reset, { flush: 'sync' });
  async function run(origin?: IntelligenceOrigin) {
    if (origin ? !canStart.value : !resumable.value) return;
    const view = player.view.value!;
    const operation = { id: crypto.randomUUID(), epoch: view.epoch },
      version = ++generation;
    pending = operation;
    busy.value = true;
    issue.value = undefined;
    cancelling.value = false;
    try {
      const result = await player.intelligence(
        origin
          ? {
              kind: 'start',
              operation_id: operation.id,
              expected_queue_revision: view.queueRevision,
              origin,
            }
          : {
              kind: 'continue',
              operation_id: operation.id,
              expected_queue_revision: view.queueRevision,
            },
      );
      if (alive && version === generation && result.intelligence === 'fallback')
        issue.value = {
          code: 'intelligence_failed',
          message: '心动模式开启失败，已返回列表循环',
          retryable: true,
        };
    } catch (e) {
      if (alive && version === generation && failure(e).code !== 'stale') issue.value = failure(e);
    } finally {
      if (alive && version === generation) {
        pending = undefined;
        busy.value = cancelling.value = false;
      }
    }
  }
  onScopeDispose(() => {
    alive = false;
    reset();
  });
  return {
    busy,
    cancelling,
    issue,
    active,
    resumable,
    canStart,
    start: (origin: IntelligenceOrigin) => run(origin),
    resume: () => run(),
    cancel,
    dismiss: () => {
      issue.value = undefined;
    },
  };
}
export type IntelligenceModel = ReturnType<typeof useIntelligence>;
