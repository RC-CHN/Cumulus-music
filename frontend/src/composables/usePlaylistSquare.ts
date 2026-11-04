import { computed, ref, shallowRef, watch } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type PlaylistSquarePage,
} from '../services/contracts.ts';

const BATCH_PAGES = 3;
export function usePlaylistSquare(
  core: Pick<CoreClient, 'playlistSquareRecommendations'>,
  account: () => AccountView,
  expired: () => void,
) {
  const groups = shallowRef<PlaylistSquarePage['groups']>([]);
  const enabled = ref<boolean>();
  const continuation = ref<string>();
  const pending = ref(false),
    error = ref(''),
    restart = ref(false);
  const pages = ref(0);
  const nextBatch = computed(
    () =>
      !!continuation.value &&
      (pages.value >= BATCH_PAGES ||
        groups.value.reduce((count, group) => count + group.items.length, 0) >= 96),
  );
  let sequence = 0;
  let failed: 'first' | 'more' = 'first';
  let opener: string | undefined;
  const seen = new Set<string>();
  function clear() {
    sequence++;
    groups.value = [];
    enabled.value = undefined;
    continuation.value = undefined;
    pending.value = false;
    error.value = '';
    restart.value = false;
    pages.value = 0;
    seen.clear();
    opener = undefined;
  }
  watch(() => [account().epoch, account().status], clear, { flush: 'sync' });
  function disable() {
    clear();
    enabled.value = false;
  }
  function observePersonalService(value: { epoch: string; value: 0 | 2 }) {
    if (account().status !== 'authenticated' || account().epoch !== value.epoch) return;
    if (value.value === 2) disable();
    else if (enabled.value === false) clear();
  }
  async function load(kind: 'first' | 'more') {
    const { epoch, status } = account();
    const cursor = kind === 'more' ? continuation.value : undefined;
    if (pending.value || status !== 'authenticated' || (kind === 'more' && !cursor)) return;
    const batch = kind === 'first' || nextBatch.value;
    const current = ++sequence;
    const live = () =>
      current === sequence && account().epoch === epoch && account().status === 'authenticated';
    pending.value = true;
    error.value = '';
    restart.value = false;
    failed = kind;
    try {
      const reply = await core.playlistSquareRecommendations(epoch, cursor);
      if (!live() || reply.epoch !== epoch) return;
      if (!reply.value.recommendationEnabled) {
        disable();
        return;
      }
      if (kind === 'first') seen.clear();
      const next = reply.value.continuation;
      if (next && seen.has(next)) {
        restart.value = true;
        error.value = '推荐未继续更新，请重新加载';
        return;
      }
      if (next) {
        seen.add(next);
        if (seen.size > 64) seen.delete(seen.keys().next().value!);
      }
      groups.value = batch ? reply.value.groups : [...groups.value, ...reply.value.groups];
      if (batch) opener = undefined;
      enabled.value = true;
      continuation.value = next ?? undefined;
      pages.value = batch ? 1 : pages.value + 1;
    } catch (value) {
      if (!live()) return;
      const issue = failure(value);
      restart.value = issue.code === 'stale';
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (live()) pending.value = false;
    }
  }
  return {
    groups,
    enabled,
    continuation,
    pending,
    error,
    nextBatch,
    load,
    observePersonalService,
    activate() {
      if (enabled.value === undefined && !error.value) void load('first');
    },
    retry: () => load(restart.value ? 'first' : failed),
    remember: (key: string) => (opener = key),
    takeOpener() {
      const key = opener;
      opener = undefined;
      return key;
    },
  };
}
