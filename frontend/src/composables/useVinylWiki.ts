import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import type {
  VinylSimilarClient,
  VinylSimilarReadState,
  VinylSimilarSeed,
  VinylWikiCard,
} from '../services/vinylSimilar.ts';

export function useVinylWiki(
  api: Pick<VinylSimilarClient, 'playerVinylWiki'>,
  seed: Readonly<Ref<VinylSimilarSeed | undefined>>,
  visible: Readonly<Ref<boolean>>,
  mounted: Readonly<Ref<boolean>> = visible,
) {
  const key = computed(() => {
    const target = seed.value;
    return target && JSON.stringify([target.epoch, target.playId, target.songId]);
  });
  const state = ref<VinylSimilarReadState>('idle');
  const card = shallowRef<VinylWikiCard | null>(null);
  let generation = 0;

  function invalidate(reset: boolean) {
    generation++;
    if (reset) card.value = null;
    if (reset || state.value === 'loading') state.value = 'idle';
  }
  async function load() {
    const current = seed.value;
    if (!current || !mounted.value || !visible.value || state.value === 'loading') return;
    const ticket = ++generation;
    const captured = key.value;
    const live = () =>
      ticket === generation && key.value === captured && mounted.value && visible.value;
    card.value = null;
    state.value = 'loading';
    try {
      const response = await api.playerVinylWiki(current.epoch, current.playId);
      if (!live()) return;
      if (response.playId !== current.playId || response.songId !== current.songId) {
        state.value = 'error';
        return;
      }
      card.value = response.card;
      state.value = 'ready';
    } catch {
      if (live()) state.value = 'error';
    }
  }

  watch(key, () => invalidate(true), { flush: 'sync' });
  watch(
    mounted,
    (active) => {
      if (!active) invalidate(true);
    },
    { flush: 'sync' },
  );
  watch(
    visible,
    (active) => {
      if (!active) invalidate(false);
    },
    { flush: 'sync' },
  );
  watch(
    [key, visible, mounted],
    ([current, active, shown]) => {
      if (current && active && shown && state.value === 'idle') void load();
    },
    { immediate: true },
  );
  onScopeDispose(() => invalidate(true));
  return { card, state, retry: load };
}

export type VinylWikiModel = ReturnType<typeof useVinylWiki>;
