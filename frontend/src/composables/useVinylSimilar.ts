import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import type {
  VinylSimilarAction,
  VinylSimilarClient,
  VinylRecommendationLane,
  VinylSimilarLane,
  VinylSimilarReadState,
  VinylSimilarSeed,
} from '../services/vinylSimilar.ts';

const keyOf = (seed?: VinylSimilarSeed) =>
  seed ? JSON.stringify([seed.epoch, seed.playId, seed.songId]) : undefined;

export function useVinylSimilar(
  api: Pick<VinylSimilarClient, 'playerVinylSimilarSongs' | 'playerVinylRelatedPlaylists'>,
  seed: Readonly<Ref<VinylSimilarSeed | undefined>>,
  visible: Readonly<Ref<boolean>>,
  play: (seed: VinylSimilarSeed, receipt: string, action: VinylSimilarAction) => Promise<boolean>,
  placement: Readonly<Ref<VinylRecommendationLane | undefined>>,
  onPlacementMismatch: () => void = () => {},
) {
  const key = computed(() =>
    seed.value && placement.value
      ? JSON.stringify([keyOf(seed.value), placement.value])
      : undefined,
  );
  const seedSong = computed(
    () => seed.value && JSON.stringify([seed.value.epoch, seed.value.songId]),
  );
  const expanded = ref(false);
  const pending = ref(false);
  const notice = ref<string>();
  let activation = 0;
  function lane<T extends { playId: string; songId: string; placement: VinylRecommendationLane }>(
    request: (epoch: string, playId: string) => Promise<T>,
  ) {
    const data = shallowRef<T>();
    const state = ref<VinylSimilarReadState>('idle');
    let serial = 0;
    function invalidate(clear: boolean) {
      serial++;
      if (clear) data.value = undefined;
      if (clear || state.value === 'loading') state.value = 'idle';
    }
    async function load() {
      const target = seed.value;
      const expectedPlacement = placement.value;
      if (!target || !expectedPlacement || !visible.value || state.value === 'loading') return;
      const requestKey = key.value;
      const requestId = ++serial;
      const live = () => serial === requestId && key.value === requestKey && visible.value;
      data.value = undefined;
      state.value = 'loading';
      try {
        const result = await request(target.epoch, target.playId);
        if (!live()) return;
        if (
          result.playId !== target.playId ||
          result.songId !== target.songId ||
          result.placement !== expectedPlacement
        ) {
          state.value = 'error';
          if (result.placement !== expectedPlacement) onPlacementMismatch();
          return;
        }
        data.value = result;
        state.value = 'ready';
      } catch {
        if (live()) state.value = 'error';
      }
    }
    return { data, state, invalidate, load };
  }
  const songs = lane((epoch, playId) => api.playerVinylSimilarSongs(epoch, playId));
  const playlists = lane((epoch, playId) => api.playerVinylRelatedPlaylists(epoch, playId));
  function invalidate(clear: boolean) {
    activation++;
    pending.value = false;
    notice.value = undefined;
    songs.invalidate(clear);
    playlists.invalidate(clear);
  }
  watch(seedSong, () => (expanded.value = false), { flush: 'sync' });
  watch(key, () => invalidate(true), { flush: 'sync' });
  watch(
    visible,
    (active) => {
      if (!active) invalidate(false);
    },
    { flush: 'sync' },
  );
  watch(
    [key, visible],
    ([current, active]) => {
      if (!current || !active) return;
      if (playlists.state.value === 'idle') void playlists.load();
      if (songs.state.value === 'idle') void songs.load();
    },
    { immediate: true },
  );
  onScopeDispose(() => invalidate(true));

  function retry(target: VinylSimilarLane) {
    if (pending.value) return;
    if (target === 'songs') {
      notice.value = undefined;
      return songs.load();
    }
    return playlists.load();
  }
  async function select(action: VinylSimilarAction) {
    const target = seed.value;
    const page = songs.data.value;
    if (
      !target ||
      !visible.value ||
      !page?.receipt ||
      page.placement !== placement.value ||
      pending.value
    )
      return;
    const index = action.kind === 'header' ? 0 : action.index;
    if (!Number.isInteger(index) || !page.songs[index]?.playable) return;
    const scope = activation;
    pending.value = true;
    notice.value = undefined;
    try {
      if (await play(target, page.receipt, action)) return;
      if (scope === activation) notice.value = '未能播放。可以重新读取相似歌曲后再试。';
    } catch {
      if (scope === activation) notice.value = '未能播放。可以重新读取相似歌曲后再试。';
    } finally {
      if (scope === activation) pending.value = false;
    }
  }
  return {
    songs: computed(() => songs.data.value?.songs ?? []),
    playlists: computed(() => playlists.data.value?.playlists ?? []),
    songsState: songs.state,
    playlistsState: playlists.state,
    canPlay: computed(() => !!songs.data.value?.receipt),
    expanded,
    pending,
    notice,
    select,
    retry,
    expand: () => {
      if (visible.value) expanded.value = true;
    },
  };
}

export type VinylSimilarModel = ReturnType<typeof useVinylSimilar>;
