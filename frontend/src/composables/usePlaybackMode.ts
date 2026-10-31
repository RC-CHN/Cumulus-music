import { computed, onScopeDispose, ref, watch } from 'vue';
import type { PlayerModel } from './usePlayer';
import type { PlaybackMode } from '../services/player';
import { failure, type CoreFailure } from '../services/contracts.ts';
import { nextModeTarget, type ModeFamily, type ModeTarget } from '../services/playbackMode.ts';
import {
  intelligenceBoundary,
  staleMode,
  type IntelligenceAttempt,
  type IntelligenceCycle,
} from './intelligenceAttempt.ts';

export function usePlaybackMode(
  player: PlayerModel,
  together: () => boolean,
  entry?: IntelligenceCycle,
) {
  const isAi = () => ['seeded', 'active'].includes(player.view.value?.intelligence ?? '');
  const family = computed<ModeFamily>(() =>
    player.view.value?.fm ? 'fm' : together() ? 'together' : isAi() ? 'intelligence' : 'ordinary',
  );
  const actual = computed(() => player.view.value?.mode ?? 'order_loop');
  const draft = ref<ModeTarget>(),
    pending = ref(false),
    issue = ref<CoreFailure>();
  const target = computed<ModeTarget>(
    () => draft.value ?? (family.value === 'intelligence' ? 'intelligence' : actual.value),
  );
  const value = computed<PlaybackMode>(() =>
    target.value === 'intelligence' ? 'order_loop' : target.value,
  );
  const displayFamily = computed<ModeFamily>(() =>
    target.value === 'intelligence'
      ? 'intelligence'
      : family.value === 'intelligence'
        ? 'ordinary'
        : family.value,
  );
  let generation = 0,
    alive = true,
    wanted: ModeTarget | undefined,
    retryTarget: ModeTarget | undefined;
  let exitingEpoch: string | undefined, enteringId: string | undefined;
  let attempt: IntelligenceAttempt | undefined,
    cancelled = false,
    scope: AbortController | undefined;
  function reset() {
    generation++;
    scope?.abort();
    scope = undefined;
    const old = attempt;
    attempt = undefined;
    if (old) void old.cancel().catch(() => {});
    wanted = retryTarget = undefined;
    draft.value = undefined;
    issue.value = undefined;
    pending.value = false;
    exitingEpoch = enteringId = undefined;
  }
  watch([() => player.view.value?.epoch, player.ready], reset, { flush: 'sync' });
  watch(
    family,
    (next, previous) => {
      if (pending.value && enteringId && enteringId === player.view.value?.intelligenceOperation)
        return;
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
      // A clock or failed durable queue write is not a successful mode recovery.
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
    const abort = new AbortController();
    scope = abort;
    let revision = source.queueRevision,
      playing = source.playId,
      intelligence = isAi();
    pending.value = true;
    try {
      while (wanted !== undefined && alive && current === generation) {
        let desired = wanted;
        wanted = undefined;
        try {
          if (desired === 'intelligence') {
            if (intelligence) continue;
            if (!entry?.eligible())
              throw {
                code: 'intelligence_origin',
                message: '当前播放来源不再支持心动模式',
                retryable: false,
              };
            if (entry.pending()) await entry.idle(source.epoch, abort.signal);
            if (!alive || current !== generation) return;
            // No request has been sent during this wait; a later click can
            // supersede the queued AI target without briefly starting a seed.
            if (wanted !== undefined) {
              desired = wanted;
              wanted = undefined;
            }
            if (desired !== 'intelligence') {
              wanted = desired;
              continue;
            }
            if (player.view.value?.queueRevision !== revision) throw staleMode();
            if (!entry.eligible())
              throw {
                code: 'intelligence_origin',
                message: '当前播放来源不再支持心动模式',
                retryable: false,
              };
            attempt = entry.enter(revision);
            enteringId = attempt.id;
            cancelled = false;
            const boundary = await intelligenceBoundary(
              player,
              attempt,
              revision,
              abort.signal,
              () => cancelled || !!attempt?.cancelRequested?.(),
            );
            if (!alive || current !== generation) return;
            attempt = undefined;
            revision = boundary.queueRevision;
            playing = boundary.playId;
            intelligence = ['seeded', 'active'].includes(boundary.intelligence ?? '');
          } else if (source.fm || intelligence || desired !== actual.value) {
            if (intelligence) exitingEpoch = source.epoch;
            const result = await player.configureMode(desired, revision, source.fm);
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
              (intelligence || desired === 'intelligence') &&
              playing !== player.view.value?.playId &&
              ['snapshot_changed', 'stale'].includes(problem.code)
            ) {
              reset();
              return;
            }
            issue.value = problem;
            retryTarget = wanted ?? desired;
            wanted = undefined;
          }
          break;
        }
      }
    } finally {
      if (alive && current === generation) {
        attempt = undefined;
        scope = undefined;
        pending.value = false;
        draft.value = undefined;
        exitingEpoch = enteringId = undefined;
      }
    }
  }
  function set(mode: ModeTarget) {
    if (!alive || !player.ready.value) return;
    wanted = mode;
    draft.value = mode;
    issue.value = undefined;
    retryTarget = undefined;
    if (attempt && mode !== 'intelligence' && !cancelled) {
      cancelled = true;
      void attempt.cancel().catch(() => {});
    }
    if (!pending.value) void drain();
  }
  function cycle() {
    set(nextModeTarget(target.value, displayFamily.value, entry?.eligible() ?? false));
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
  return { family, displayFamily, actual, target, value, pending, issue, canRetry, cycle, retry };
}
