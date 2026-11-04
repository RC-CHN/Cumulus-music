import { ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import type {
  PodcastRecentClient,
  PodcastRecentPage,
  PodcastRecentQuery,
} from '../services/podcast.ts';

export function usePodcastRecent(
  account: () => AccountView,
  expired: () => void,
  core: PodcastRecentClient = useCore(),
) {
  const page = shallowRef<PodcastRecentPage>();
  const pending = ref(false);
  const issue = ref('');
  const scrollTop = ref(0);
  const radioOpener = ref<string>();
  let request = 0;
  let last: PodcastRecentQuery = { kind: 'open' };
  watch(
    () => [account().epoch, account().status],
    () => {
      request++;
      page.value = undefined;
      pending.value = false;
      issue.value = '';
      scrollTop.value = 0;
      radioOpener.value = undefined;
      last = { kind: 'open' };
    },
    { flush: 'sync' },
  );

  async function load(query: PodcastRecentQuery) {
    const { epoch, status } = account();
    if (status !== 'authenticated' || pending.value) return;
    const id = ++request;
    const live = () =>
      request === id && account().epoch === epoch && account().status === 'authenticated';
    pending.value = true;
    issue.value = '';
    last = query;
    try {
      const result = await core.podcastRecent(epoch, query);
      if (!live() || result.epoch !== epoch) return;
      page.value = result.value;
      if (query.kind === 'refresh') scrollTop.value = 0;
    } catch (error) {
      if (!live()) return;
      const result = failure(error);
      issue.value = result.message;
      if (result.code === 'session_expired' || result.code === 'profile_changed') expired();
      if (result.code === 'stale' || result.code === 'queue_changed') last = { kind: 'open' };
    } finally {
      if (live()) pending.value = false;
    }
  }

  function open() {
    if (!page.value && !issue.value) void load({ kind: 'open' });
  }
  function next() {
    if (page.value?.hasMore && !issue.value)
      void load({ kind: 'next', revision: page.value.revision });
  }

  return {
    page,
    pending,
    issue,
    scrollTop,
    radioOpener,
    open,
    next,
    refresh: () => load({ kind: 'refresh' }),
    retry: () => load(last),
  };
}
export type PodcastRecentModel = ReturnType<typeof usePodcastRecent>;
