import { computed, onScopeDispose, ref, watch } from 'vue';
import type { PlayerModel } from './usePlayer';
import type { IntelligenceOrigin } from '../services/player.ts';
import { failure, type CoreFailure, type CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { intelligenceIdle, type IntelligenceAttempt } from './intelligenceAttempt.ts';

export function useIntelligence(
  player: PlayerModel,
  together: () => boolean,
  core: CoreClient = useCore(),
) {
  const busy = ref(false),
    cancelling = ref(false),
    issue = ref<CoreFailure>();
  const progress = ref<'preparing' | 'recommendation'>();
  let pending: { id: string; epoch: string; cancelled: boolean } | undefined,
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
    operation.cancelled = true;
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
    progress.value = undefined;
    issue.value = undefined;
    if (operation)
      void core.playerIntelligenceCancel(operation.epoch, operation.id).catch(() => {});
  }
  watch([() => player.view.value?.epoch, player.ready], reset, { flush: 'sync' });
  function begin(
    origin?: IntelligenceOrigin,
    revision?: string,
    mode = false,
  ): IntelligenceAttempt | undefined {
    if (origin ? !canStart.value : !resumable.value) return;
    const view = player.view.value!;
    const operation = { id: crypto.randomUUID(), epoch: view.epoch, cancelled: false },
      version = ++generation;
    pending = operation;
    busy.value = true;
    progress.value = origin ? 'preparing' : 'recommendation';
    issue.value = undefined;
    cancelling.value = false;
    const attempt: IntelligenceAttempt = {
      ...operation,
      cancelRequested: () => operation.cancelled,
      promise: execute(),
      cancel: async () => {
        if (pending === operation) await cancel();
      },
    };
    return attempt;
    async function execute() {
      let entered = false;
      const stop = watch(
        player.view,
        (next) => {
          if (
            alive &&
            version === generation &&
            next?.epoch === operation.epoch &&
            next.intelligenceOperation === operation.id &&
            next.intelligence
          ) {
            entered = true;
            progress.value = 'recommendation';
          }
        },
        { flush: 'sync' },
      );
      try {
        const result = await player.intelligence(
          origin
            ? {
                kind: 'start',
                operation_id: operation.id,
                expected_queue_revision: revision ?? view.queueRevision,
                origin,
              }
            : {
                kind: 'continue',
                operation_id: operation.id,
                expected_queue_revision: revision ?? view.queueRevision,
              },
        );
        const current = player.view.value;
        if (
          alive &&
          version === generation &&
          result.intelligence === 'fallback' &&
          current?.epoch === result.epoch &&
          current.queueRevision === result.queueRevision &&
          current.intelligenceOperation === result.intelligenceOperation
        )
          issue.value = {
            code: 'intelligence_failed',
            message: '心动模式开启失败，已返回列表循环',
            retryable: true,
          };
        return result;
      } catch (e) {
        const current = player.view.value;
        const stillCurrent = entered
          ? current?.intelligenceOperation === operation.id
          : current?.playId === view.playId;
        if (
          alive &&
          version === generation &&
          stillCurrent &&
          (!mode || entered) &&
          failure(e).code !== 'stale'
        )
          issue.value = failure(e);
        throw e;
      } finally {
        stop();
        if (alive && version === generation) {
          pending = undefined;
          busy.value = cancelling.value = false;
          progress.value = undefined;
        }
      }
    }
  }
  async function run(origin?: IntelligenceOrigin) {
    const attempt = begin(origin);
    if (attempt)
      try {
        return await attempt.promise;
      } catch {
        /* The operation-specific feedback is already recorded. */
      }
  }
  onScopeDispose(() => {
    alive = false;
    reset();
  });
  return {
    busy,
    progress,
    cancelling,
    issue,
    active,
    resumable,
    canStart,
    enterCurrent: (revision: string) => {
      const attempt = begin({ kind: 'current' }, revision, true);
      if (!attempt)
        throw { code: 'busy', message: '心动模式正在切换，请稍后再试', retryable: true };
      return attempt;
    },
    idle: (epoch: string, signal: AbortSignal) => intelligenceIdle(busy, player, epoch, signal),
    start: (origin: IntelligenceOrigin) => run(origin),
    resume: () => run(),
    cancel,
    dismiss: () => {
      issue.value = undefined;
    },
  };
}
export type IntelligenceModel = ReturnType<typeof useIntelligence>;
