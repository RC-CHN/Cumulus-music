import { ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import type { PodcastHomeBlock } from '../services/podcast';

export function usePodcastHome(account: () => AccountView, expired: () => void) {
  const core = useCore();
  const blocks = shallowRef<PodcastHomeBlock[]>([]);
  const pending = ref(false);
  const error = ref('');
  const enabled = ref<boolean>();
  const continuation = ref<string>();
  const restart = ref(false);
  let pages = 0;
  let request = 0;
  const seen = new Set<string>();
  function clear() {
    request++;
    blocks.value = [];
    enabled.value = undefined;
    continuation.value = undefined;
    restart.value = false;
    pending.value = false;
    error.value = '';
    seen.clear();
    pages = 0;
  }
  watch(() => [account().epoch, account().status], clear, { flush: 'sync' });
  async function load(refresh = false) {
    const { epoch, status } = account();
    if (pending.value || status !== 'authenticated') return;
    if (refresh || restart.value) clear();
    if (enabled.value !== undefined && !continuation.value) return;
    const id = ++request;
    const live = () =>
      request === id && account().epoch === epoch && account().status === 'authenticated';
    pending.value = true;
    error.value = '';
    try {
      const result = await core.podcastHome(epoch, continuation.value);
      if (!live() || result.epoch !== epoch) return;
      const next = result.value.continuation;
      if (next && seen.has(next)) {
        restart.value = true;
        error.value = '播客推荐未继续更新，请重新读取';
        return;
      }
      if (next) seen.add(next);
      enabled.value = result.value.recommendationEnabled;
      blocks.value = [...blocks.value, ...result.value.blocks];
      continuation.value = next ?? undefined;
      if (++pages >= 12 && next) {
        continuation.value = undefined;
        restart.value = true;
        error.value = '本次推荐已达到读取上限';
      }
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
  function activate() {
    if (enabled.value === undefined && !error.value) void load();
  }
  return { blocks, pending, error, enabled, continuation, restart, load, activate };
}
export type PodcastHomeModel = ReturnType<typeof usePodcastHome>;
