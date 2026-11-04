import { computed, shallowRef, watch, type Ref } from 'vue';
import { useCore } from '../services/core';
import type { SongDownloadState } from '../services/downloads';
import type { CurrentCatalogTrack } from '../services/player';

export function usePlaybackDownloadState(
  target: Readonly<Ref<CurrentCatalogTrack | undefined>>,
  changing: () => boolean,
) {
  const core = useCore();
  const key = computed(() => {
    const value = target.value;
    return value ? JSON.stringify([value.epoch, value.playId, value.track.id]) : undefined;
  });
  const result = shallowRef<{ key: string; state: SongDownloadState }>();
  watch(
    [key, changing],
    ([identity, busy], _previous, onCleanup) => {
      if (result.value?.key !== identity) result.value = undefined;
      if (!identity || busy) return;
      const current = target.value;
      if (!current) return;
      const selected = { key: identity, epoch: current.epoch, id: current.track.id };
      let active = true;
      let timer: ReturnType<typeof setTimeout> | undefined;
      onCleanup(() => {
        active = false;
        clearTimeout(timer);
      });
      async function observe() {
        try {
          const state = await core.songDownloadState(selected.epoch, selected.id);
          if (active && key.value === selected.key) result.value = { key: selected.key, state };
        } catch {
          // A failed optional local read cannot block playback or download setup.
          if (active && key.value === selected.key && !result.value)
            result.value = { key: selected.key, state: 'none' };
        }
        if (
          active &&
          key.value === identity &&
          (result.value?.state === 'working' || result.value?.state === 'waiting')
        )
          timer = setTimeout(() => void observe(), 900);
      }
      void observe();
    },
    { immediate: true },
  );
  return computed(() => {
    const value = result.value;
    return value?.key === key.value ? value?.state : undefined;
  });
}
