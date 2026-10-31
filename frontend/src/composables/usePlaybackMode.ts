import { computed, onScopeDispose, ref, watch } from 'vue';
import type { PlayerModel } from './usePlayer';
import type { PlaybackMode } from '../services/player';
import { failure, type CoreFailure } from '../services/contracts.ts';
import { nextMode, type ModeFamily } from '../services/playbackMode.ts';

export function usePlaybackMode(player: PlayerModel, together: () => boolean) {
  const family = computed<ModeFamily>(() =>
    player.view.value?.fm ? 'fm' : together() ? 'together' : 'ordinary',
  );
  const actual = computed(() => player.view.value?.mode ?? 'order_loop');
  const draft = ref<PlaybackMode>(),
    pending = ref(false),
    issue = ref<CoreFailure>();
  const value = computed(() => draft.value ?? actual.value);
  let generation = 0,
    alive = true,
    wanted: PlaybackMode | undefined,
    retryTarget: PlaybackMode | undefined;
  function reset() {
    generation++;
    wanted = retryTarget = undefined;
    draft.value = undefined;
    issue.value = undefined;
    pending.value = false;
  }
  watch([() => player.view.value?.epoch, player.ready, family], reset, { flush: 'sync' });
  watch(
    () => player.view.value?.queueRevision,
    () => {
      // An independent queue/mode operation supersedes an old failed target.
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
    pending.value = true;
    try {
      while (wanted !== undefined && alive && current === generation) {
        const target = wanted;
        wanted = undefined;
        try {
          if (source.fm || target !== actual.value) {
            const result = await player.configureMode(target, revision, source.fm);
            if (!alive || current !== generation) return;
            if (result.queueRevision !== player.view.value?.queueRevision) {
              wanted = undefined;
              break;
            }
            revision = result.queueRevision;
          }
        } catch (error) {
          if (alive && current === generation) {
            issue.value = failure(error);
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
  const cycle = () => set(nextMode(value.value, family.value));
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
  return { family, actual, value, pending, issue, canRetry, cycle, retry };
}
