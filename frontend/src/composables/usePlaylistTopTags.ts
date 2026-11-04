import { ref, shallowRef, watch } from 'vue';
import { failure, type AccountView, type CoreClient } from '../services/contracts.ts';

export function usePlaylistTopTags(
  core: Pick<CoreClient, 'playlistTopTags'>,
  account: () => AccountView,
  expired: () => void,
) {
  const tags = shallowRef<string[]>([]);
  const enabled = ref<boolean>(),
    pending = ref(false),
    error = ref('');
  let generation = 0;
  function clear() {
    generation++;
    tags.value = [];
    enabled.value = undefined;
    pending.value = false;
    error.value = '';
  }
  watch(() => [account().epoch, account().status], clear, { flush: 'sync' });
  function disable() {
    clear();
    enabled.value = false;
  }
  function observePersonalService(observation: { epoch: string; value: 0 | 2 }) {
    if (account().status !== 'authenticated' || account().epoch !== observation.epoch) return;
    if (observation.value === 2) disable();
    else if (enabled.value === false) clear();
  }
  async function load() {
    const owner = account().epoch;
    if (enabled.value !== undefined || pending.value || account().status !== 'authenticated')
      return;
    const ticket = ++generation;
    const live = () =>
      ticket === generation && account().epoch === owner && account().status === 'authenticated';
    pending.value = true;
    error.value = '';
    try {
      const result = await core.playlistTopTags(owner);
      if (!live() || result.epoch !== owner) return;
      if (!result.value.recommendationEnabled) disable();
      else {
        enabled.value = true;
        tags.value = result.value.tags;
      }
    } catch (value) {
      if (!live()) return;
      const issue = failure(value);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (live()) pending.value = false;
    }
  }
  return {
    tags,
    error,
    pending,
    load,
    observePersonalService,
    activate() {
      if (!error.value) void load();
    },
  };
}
