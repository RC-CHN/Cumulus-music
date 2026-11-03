import { computed, shallowRef, watch, type Ref } from 'vue';
import { useCore } from '../services/core';
import type { PlayerView } from '../services/player';
import { chorusMarkerPercent } from '../services/seekRail';

export function usePlaybackChorus(
  view: Ref<PlayerView | undefined>,
  loaded: Readonly<Ref<boolean>>,
) {
  const core = useCore();
  const target = computed(() => {
    const current = view.value;
    return loaded.value &&
      current?.resourceKind === 'catalog' &&
      current.playId &&
      current.timeline?.end != null
      ? JSON.stringify([current.epoch, current.playId])
      : undefined;
  });
  const result = shallowRef<{ key: string; seconds: number }>();
  watch(
    target,
    async (key, _old, onCleanup) => {
      result.value = undefined;
      if (!key) return;
      let active = true;
      onCleanup(() => (active = false));
      const [epoch, playId] = JSON.parse(key) as [string, string];
      try {
        const seconds = await core.playerChorus(epoch, playId);
        if (active && key === target.value && seconds != null) result.value = { key, seconds };
      } catch {
        // Missing metadata never changes the player state or creates a retry prompt.
      }
    },
    { immediate: true },
  );
  return computed(() => {
    if (!result.value || result.value.key !== target.value) return null;
    return chorusMarkerPercent(view.value?.timeline, result.value.seconds);
  });
}
