import { ref, shallowRef, watch } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type PlaylistTagGroup,
} from '../services/contracts.ts';

export function usePlaylistTagCatalogue(
  core: Pick<CoreClient, 'playlistTagCatalogue'>,
  account: () => AccountView,
  expired: () => void,
) {
  const groups = shallowRef<PlaylistTagGroup[]>();
  const pending = ref(false);
  const error = ref('');
  let generation = 0;
  watch(
    () => [account().epoch, account().status],
    () => {
      generation++;
      groups.value = undefined;
      pending.value = false;
      error.value = '';
    },
    { flush: 'sync' },
  );
  async function load() {
    if (groups.value || pending.value || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    const ticket = ++generation;
    const current = () =>
      ticket === generation && account().epoch === epoch && account().status === 'authenticated';
    pending.value = true;
    error.value = '';
    try {
      const result = await core.playlistTagCatalogue(epoch);
      if (current() && result.epoch === epoch) groups.value = result.value;
    } catch (value) {
      if (!current()) return;
      const issue = failure(value);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (current()) pending.value = false;
    }
  }
  return { groups, pending, error, load };
}
