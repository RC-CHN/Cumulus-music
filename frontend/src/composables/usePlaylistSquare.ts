import { ref, shallowRef, watch } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type PlaylistSquarePage,
} from '../services/contracts.ts';

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
  const manual = ref(false);
  let sequence = 0;
  let failed: 'first' | 'more' = 'first';
  let opener: string | undefined;
  let scroll = 0;
  const horizontal = new Map<number, number>();
  const seen = new Set<string>();
  function clear() {
    sequence++;
    groups.value = [];
    enabled.value = undefined;
    continuation.value = undefined;
    pending.value = false;
    error.value = '';
    restart.value = false;
    manual.value = false;
    seen.clear();
    opener = undefined;
    scroll = 0;
    horizontal.clear();
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
    const replace = kind === 'first';
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
      groups.value = replace ? reply.value.groups : [...groups.value, ...reply.value.groups];
      if (replace) {
        opener = undefined;
        scroll = 0;
        horizontal.clear();
      }
      manual.value = reply.value.groups.length === 0;
      enabled.value = true;
      continuation.value = next ?? undefined;
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
    manual,
    load,
    observePersonalService,
    activate() {
      if (enabled.value === undefined && !error.value) void load('first');
    },
    retry: () => load(restart.value ? 'first' : failed),
    remember: (key: string) => (opener = key),
    saveScroll: (value: number) => (scroll = value),
    saveRail: (index: number, left: number) => horizontal.set(index, left),
    rail: (index: number) => horizontal.get(index) ?? 0,
    restore() {
      const result = { scroll, opener };
      opener = undefined;
      return result;
    },
  };
}
