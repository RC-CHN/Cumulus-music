import { computed, ref, watch } from 'vue';
import type { CoreClient, PlaylistSubscribersPage } from '../services/contracts';

export function usePlaylistSubscribers(
  client: Pick<CoreClient, 'playlistSubscribers'>,
  epoch: () => string,
  playlistId: () => string | null,
) {
  const open = ref(false);
  const busy = ref(false);
  const failed = ref(false);
  const value = ref<PlaylistSubscribersPage | null>(null);
  const requestedOffset = ref(0);
  const hasPrevious = computed(() => value.value?.previousOffset != null);
  let generation = 0;

  function hide() {
    generation++;
    open.value = busy.value = failed.value = false;
    value.value = null;
    requestedOffset.value = 0;
  }

  watch([epoch, playlistId], hide, { flush: 'sync' });

  async function read(offset: number) {
    const id = playlistId();
    if (!open.value || !id || busy.value) return;
    const owner = epoch();
    const ticket = ++generation;
    requestedOffset.value = offset;
    busy.value = true;
    failed.value = false;
    try {
      const response = await client.playlistSubscribers(owner, id, offset);
      if (ticket !== generation) return;
      if (response.epoch !== owner) {
        failed.value = true;
        return;
      }
      value.value = response.value;
    } catch {
      if (ticket === generation) failed.value = true;
    } finally {
      if (ticket === generation) busy.value = false;
    }
  }

  function show() {
    if (open.value || !playlistId()) return;
    open.value = true;
    return read(0);
  }

  return {
    open,
    busy,
    failed,
    value,
    hasPrevious,
    show,
    hide,
    retry: () => read(requestedOffset.value),
    next: () => {
      const offset = value.value?.nextOffset;
      if (offset != null) return read(offset);
    },
    previous: () => {
      const offset = value.value?.previousOffset;
      if (offset != null) return read(offset);
    },
  };
}
