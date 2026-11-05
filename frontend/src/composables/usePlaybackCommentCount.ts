import { computed, shallowRef, watch, type Ref } from 'vue';
import { useCore } from '../services/core.ts';
import type { PlayerClient, PlayerView } from '../services/player.ts';

export function usePlaybackCommentInfo(
  view: Ref<PlayerView | undefined>,
  loaded: Readonly<Ref<boolean>>,
  accountEpoch: () => string,
  core: Pick<PlayerClient, 'playerCommentCount'> = useCore(),
) {
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
  let receipts = new Set<string>();
  watch(
    target,
    async (key, _, onCleanup) => {
      result.value = undefined;
      receipts = new Set<string>();
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
  const count = computed(() => {
    const current = result.value;
    return current && current.key === target.value ? current.count : undefined;
  });
  function notePublished(key: string, receiptId: string) {
    const current = result.value;
    if (key !== target.value || current?.key !== key || receipts.has(receiptId)) return;
    receipts.add(receiptId);
    if (Number.isSafeInteger(current.count + 1)) result.value = { key, count: current.count + 1 };
  }
  return { count, notePublished };
}
