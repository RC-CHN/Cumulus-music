import { computed, onScopeDispose, ref, watch } from 'vue';
import type { PlayerModel } from './usePlayer';
import type { PlaybackMode } from '../services/player';
import { failure, type CoreFailure } from '../services/contracts.ts';
import { nextMode, type ModeFamily } from '../services/playbackMode.ts';

export function usePlaybackMode(player: PlayerModel, together: () => boolean) {
  const family = computed<ModeFamily>(() =>
    player.view.value?.fm
      ? 'fm'
      : together()
        ? 'together'
        : ['seeded', 'active'].includes(player.view.value?.intelligence ?? '')
          ? 'intelligence'
          : 'ordinary',
  );
  const actual = computed(() => player.view.value?.mode ?? 'order_loop');
  const draft = ref<PlaybackMode>(),
    pending = ref(false),
    issue = ref<CoreFailure>();
  const value = computed(() => draft.value ?? actual.value);
  const displayFamily = computed(() =>
    family.value === 'intelligence' && draft.value !== undefined ? 'ordinary' : family.value,
  );
  let generation = 0,
    alive = true,
    wanted: PlaybackMode | undefined,
    retryTarget: PlaybackMode | undefined,
    exitingEpoch: string | undefined;
  function reset() {
    generation++;
    wanted = retryTarget = undefined;
    draft.value = undefined;
    issue.value = undefined;
    pending.value = false;
    exitingEpoch = undefined;
  }
  watch([() => player.view.value?.epoch, player.ready], reset, { flush: 'sync' });
  watch(
    family,
    (next, previous) => {
      // The successful AI exit changes its family before the command reply is
      // observed. Keep the user's latest ordinary target in the same drain.
      if (
        previous === 'intelligence' &&
        next === 'ordinary' &&
        pending.value &&
        exitingEpoch === player.view.value?.epoch
      )
        return;
      reset();
    },
    { flush: 'sync' },
  );
  watch(
    [() => player.view.value?.playId, actual],
    () => {
      // A failed durable queue write also advances its native revision. That
      // delayed observation is not recovery. Only a changed media identity or
      // confirmed mode supersedes an old failed target here.
      if (!pending.value) {
        issue.value = undefined;
        retryTarget = undefined;
      }
    },
    { flush: 'sync' },
  );
  async function drain() {
    const current = generation,
      source = player.view.value;
    if (!source) return;
    let revision = source.queueRevision;
    let intelligence = ['seeded', 'active'].includes(source.intelligence ?? '');
    pending.value = true;
    try {
      while (wanted !== undefined && alive && current === generation) {
        const target = wanted;
        wanted = undefined;
        try {
          if (source.fm || intelligence || target !== actual.value) {
            if (intelligence) exitingEpoch = source.epoch;
            const result = await player.configureMode(target, revision, source.fm);
            if (!alive || current !== generation) return;
            if (result.queueRevision !== player.view.value?.queueRevision) {
              wanted = undefined;
              break;
            }
            revision = result.queueRevision;
            intelligence = false;
          }
        } catch (error) {
          if (alive && current === generation) {
            const problem = failure(error);
            if (
              intelligence &&
              family.value === 'ordinary' &&
              source.playId !== player.view.value?.playId &&
              ['snapshot_changed', 'stale'].includes(problem.code)
            ) {
              reset();
              return;
            }
            issue.value = problem;
            retryTarget = wanted ?? target;
            wanted = undefined;
          }
          break;
        }
      }
    } finally {
      if (alive && current === generation) {
        pending.value = false;
        draft.value = undefined;
        exitingEpoch = undefined;
      }
    }
  }
  function set(mode: PlaybackMode) {
    if (!alive || !player.ready.value) return;
    wanted = mode;
    draft.value = mode;
    issue.value = undefined;
    retryTarget = undefined;
    if (!pending.value) void drain();
  }
  const cycle = () => set(nextMode(value.value, displayFamily.value));
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
  return { family, displayFamily, actual, value, pending, issue, canRetry, cycle, retry };
}
