import { computed, ref, shallowRef } from 'vue';
import type { CoreClient, PlaylistCreatorProfile } from '../services/contracts';

export function usePlaylistCreator(client: Pick<CoreClient, 'playlistCreatorProfile'>) {
  const profile = shallowRef<PlaylistCreatorProfile | null>();
  const busy = ref(false);
  const failed = ref(false);
  const group = ref<'created' | 'collected'>('created');
  const page = ref(0);
  const opened = ref('');
  const list = computed(() => profile.value?.[group.value] ?? []);
  const cards = computed(() => list.value.slice(page.value * 24, (page.value + 1) * 24));
  const pages = computed(() => Math.max(1, Math.ceil(list.value.length / 24)));
  let generation = 0;
  let epoch = '';
  let uid = '';
  let active = false;

  async function load() {
    if (!active || busy.value) return;
    const ticket = ++generation;
    busy.value = true;
    failed.value = false;
    try {
      const reply = await client.playlistCreatorProfile(epoch, uid);
      if (ticket !== generation) return;
      if (reply.epoch !== epoch || (reply.value && reply.value.id !== uid)) {
        failed.value = true;
        return;
      }
      profile.value = reply.value;
    } catch {
      if (ticket === generation) failed.value = true;
    } finally {
      if (ticket === generation) busy.value = false;
    }
  }
  function enter(owner: string, target: string | null) {
    generation++;
    busy.value = failed.value = false;
    active = !!owner && !!target;
    if (owner !== epoch || (target && target !== uid)) {
      epoch = owner;
      uid = target ?? '';
      profile.value = undefined;
      group.value = 'created';
      opened.value = '';
      page.value = 0;
    }
    if (active && profile.value === undefined) return load();
  }
  function select(value: typeof group.value) {
    if (value === group.value) return;
    group.value = value;
    page.value = 0;
    opened.value = '';
  }
  return { profile, busy, failed, group, page, pages, cards, opened, enter, load, select };
}
