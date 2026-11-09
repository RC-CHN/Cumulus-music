import { computed, shallowRef, watch, type Ref } from 'vue';
import { useCore } from '../services/core.ts';
import type { PlayerClient, PlayerView } from '../services/player.ts';
import { chorusMarkerPercent } from '../services/seekRail.ts';

export function usePlaybackChorus(
  view: Ref<PlayerView | undefined>,
  loaded: Readonly<Ref<boolean>>,
  core: Pick<PlayerClient, 'playerChorus'> = useCore(),
) {
  const target = computed(() => {
    const current = view.value;
    return loaded.value &&
      current?.resourceKind === 'catalog' &&
      current.playId &&
      current.timeline?.end != null
      ? JSON.stringify([current.epoch, current.playId])
      : undefined;
  });
  const observation = computed(() => {
    const current = view.value;
    const key = target.value;
    return key && current
      ? JSON.stringify([
          key,
          !current.fm && !current.intelligence && !current.catalogFileSource && current.queueLen > 1
            ? current.queueRevision
            : null,
        ])
      : undefined;
  });
  const result = shallowRef<{ key: string; seconds: number }>();
  watch(
    observation,
    async (observed, _old, onCleanup) => {
      const key = target.value;
      if (result.value?.key !== key) result.value = undefined;
      if (!key || !observed) return;
      let active = true;
      onCleanup(() => (active = false));
      const [epoch, playId] = JSON.parse(key) as [string, string];
      try {
        const seconds = await core.playerChorus(epoch, playId);
        if (active && observed === observation.value)
          result.value = seconds == null ? undefined : { key, seconds };
      } catch {
        // Missing metadata never changes the player state or creates a retry prompt.
      }
    },
    { immediate: true },
  );
  return computed(() => {
    if (!result.value || result.value.key !== target.value) return null;
    const seconds = result.value.seconds;
    const percent = chorusMarkerPercent(view.value?.timeline, seconds);
    return percent === null ? null : { seconds, percent };
  });
}
