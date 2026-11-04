import { computed, ref, shallowRef, watch, type Ref } from 'vue';
import { useCore } from '../services/core';
import type { SongDownloadState } from '../services/downloads';
import type { CurrentPlaybackDownload } from '../services/player';

export function usePlaybackDownloadState(
  target: Readonly<Ref<CurrentPlaybackDownload | undefined>>,
  changing: () => boolean,
) {
  const core = useCore();
  const key = computed(() => {
    const value = target.value;
    return value ? JSON.stringify([value.epoch, value.playId, value.kind, value.id]) : undefined;
  });
  const result = shallowRef<{ key: string; state: SongDownloadState | 'unknown' }>();
  const attempt = ref(0);
  watch(
    [key, changing, attempt],
    ([identity, busy], _previous, onCleanup) => {
      // A download command can create or remove a task. Do not reveal a
      // pre-command "none" while the follow-up SQLite read is in flight.
      if (busy || result.value?.key !== identity) result.value = undefined;
      if (!identity || busy) return;
      const current = target.value;
      if (!current) return;
      const selected = { key: identity, epoch: current.epoch, id: current.id, kind: current.kind };
      let active = true;
      let timer: ReturnType<typeof setTimeout> | undefined;
      onCleanup(() => {
        active = false;
        clearTimeout(timer);
      });
      async function observe() {
        try {
          const state = await (selected.kind === 'voice'
            ? core.voiceDownloadState(selected.epoch, selected.id)
            : core.songDownloadState(selected.epoch, selected.id));
          if (active && key.value === selected.key) result.value = { key: selected.key, state };
        } catch {
          if (active && key.value === selected.key && !result.value)
            result.value = { key: selected.key, state: 'unknown' };
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
  const state = computed(() => {
    const value = result.value;
    return !changing() && value?.key === key.value ? value?.state : undefined;
  });
  function retry() {
    if (state.value !== 'unknown') return;
    result.value = undefined;
    attempt.value++;
  }
  return { state, retry };
}
