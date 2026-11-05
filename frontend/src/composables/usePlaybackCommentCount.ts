import { computed, shallowRef, watch, type Ref } from 'vue';
import { useCore } from '../services/core';
import type { PlayerView } from '../services/player';

export function usePlaybackCommentInfo(
  view: Ref<PlayerView | undefined>,
  loaded: Readonly<Ref<boolean>>,
  accountEpoch: () => string,
) {
  const core = useCore();
  const target = computed(() => {
    const current = view.value;
    return loaded.value &&
      (current?.resourceKind === 'catalog' || current?.resourceKind === 'voice') &&
      current.epoch === accountEpoch() &&
      current.playId
      ? JSON.stringify([current.epoch, current.playId])
      : undefined;
  });
  const result = shallowRef<{ key: string; count: number }>();
  watch(
    target,
    async (key, _, onCleanup) => {
      result.value = undefined;
      if (!key) return;
      let active = true;
      onCleanup(() => (active = false));
      const [epoch, playId] = JSON.parse(key) as [string, string];
      try {
        const count = await core.playerCommentCount(epoch, playId);
        if (active && key === target.value && count != null) result.value = { key, count };
      } catch {
        // Optional metadata never interrupts playback or comment navigation.
      }
    },
    { immediate: true },
  );
  return computed(() => {
    const current = result.value;
    return current && current.key === target.value ? current.count : undefined;
  });
}
