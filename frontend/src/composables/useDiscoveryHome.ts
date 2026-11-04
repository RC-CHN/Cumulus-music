import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import type { DiscoveryHomeClient, DiscoveryHomePage } from '../services/discoveryHome';
import type { RankingGroup } from '../services/rankings';

// M/32.n uses 3h when the preload config has no home override.
const FRESH_MS = 3 * 60 * 60 * 1000 - 3000;

export function useDiscoveryHome(
  core: DiscoveryHomeClient,
  account: () => AccountView,
  active: () => boolean,
  expired: () => void,
) {
  const data = shallowRef<DiscoveryHomePage>();
  const ranking = shallowRef<RankingGroup>();
  const error = ref('');
  const loading = ref(false);
  const viewport = { scroll: 0, focus: '', rails: new Map<string, number>() };
  let owner: string | undefined;
  let serial = 0;
  let observed = 0;
  let rankingObserved = 0;
  let rankingFlight = 0;
  let rankingSerial = 0;
  let rankTimer: ReturnType<typeof setInterval> | undefined;
  function reset(epoch?: string) {
    ++serial;
    owner = epoch;
    data.value = undefined;
    ranking.value = undefined;
    rankingObserved = 0;
    rankingFlight = 0;
    error.value = '';
    loading.value = false;
    observed = 0;
    viewport.scroll = 0;
    viewport.focus = '';
    viewport.rails.clear();
  }
  async function load(force = false) {
    if (!owner || !active() || loading.value) return;
    if (!force && observed && Date.now() - observed < FRESH_MS) return;
    const epoch = owner;
    const request = ++serial;
    loading.value = true;
    error.value = '';
    try {
      const response = await core.discoveryHome(epoch);
      if (owner !== epoch || request !== serial || response.epoch !== epoch) return;
      data.value = response.value;
      observed = Date.now();
      if (!response.value.recommendationEnabled) {
        viewport.scroll = 0;
        viewport.focus = '';
        viewport.rails.clear();
      }
    } catch (e) {
      if (owner !== epoch || request !== serial) return;
      const issue = failure(e);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
        reset();
        if (active()) expired();
      }
    } finally {
      if (request === serial) loading.value = false;
    }
  }
  async function loadRanking(force = false) {
    if (!owner || !active() || rankingFlight) return;
    if (!force && rankingObserved && Date.now() - rankingObserved < FRESH_MS) return;
    const epoch = owner;
    const request = ++rankingSerial;
    rankingFlight = request;
    try {
      const response = await core.discoveryHomeRankings(epoch);
      if (owner !== epoch || request !== rankingFlight || response.epoch !== epoch) return;
      ranking.value = response.value ?? undefined;
      rankingObserved = Date.now();
    } catch (e) {
      if (owner !== epoch || request !== rankingFlight) return;
      if (['session_expired', 'profile_changed'].includes(failure(e).code)) {
        reset();
        if (active()) expired();
      }
    } finally {
      if (owner === epoch && request === rankingFlight) rankingFlight = 0;
    }
  }
  watch(
    [() => account().epoch, () => account().status, active],
    ([epoch, status, visible]) => {
      const key = status === 'authenticated' ? epoch : undefined;
      if (owner !== key) reset(key);
      clearInterval(rankTimer);
      if (key && visible) {
        void load();
        void loadRanking();
        rankTimer = setInterval(() => void loadRanking(), 3 * 60 * 60 * 1000);
      }
    },
    { immediate: true, flush: 'sync' },
  );
  onScopeDispose(() => clearInterval(rankTimer));
  function observePreference(enabled: boolean) {
    if (!owner || data.value?.recommendationEnabled === enabled) return;
    if (!enabled) {
      reset(owner);
      data.value = { recommendationEnabled: false, sections: [] };
      observed = Date.now();
    } else {
      observed = 0;
      if (active()) {
        void load();
        void loadRanking();
      }
    }
  }
  return {
    data,
    ranking,
    loading,
    error,
    viewport,
    refresh: () => {
      void loadRanking(true);
      return load(true);
    },
    observePreference,
  };
}

export type DiscoveryHomeModel = ReturnType<typeof useDiscoveryHome>;
