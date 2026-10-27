import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { RankingClient, RankingQuery, RankingView } from '../services/rankings.ts';
import { useCore } from '../services/core.ts';

export function useRankings(core: RankingClient = useCore(), expired = () => {}) {
  const view = shallowRef<RankingView>();
  const query = shallowRef<RankingQuery>({ kind: 'home' });
  const loading = ref(false);
  const error = ref('');
  let epoch: string | undefined;
  let serial = 0;
  let disposed = false;
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    view.value = undefined;
    loading.value = false;
    error.value = '';
  }
  async function open(target: RankingQuery, refresh = false) {
    const same = JSON.stringify(target) === JSON.stringify(query.value);
    if (!refresh && same && loading.value) return;
    const current = ++serial;
    const owner = epoch;
    query.value = target;
    if (!same) view.value = undefined;
    error.value = '';
    if (!owner || disposed) return;
    loading.value = true;
    const active = () => !disposed && current === serial && owner === epoch;
    try {
      if (!refresh) {
        try {
          const saved = await core.rankingPage(owner, target, false);
          if (!active()) return;
          if (saved.epoch !== owner) return;
          view.value = saved.value;
          if (saved.value.fresh) return;
        } catch (e) {
          if (!active()) return;
          if (['session_expired', 'profile_changed'].includes(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      const result = await core.rankingPage(owner, target, true);
      if (active() && result.epoch === owner) view.value = result.value;
    } catch (e) {
      if (!active()) return;
      const issue = failure(e);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) {
        view.value = undefined;
        expired();
      }
    } finally {
      if (active()) loading.value = false;
    }
  }
  function dispose() {
    disposed = true;
    serial++;
  }
  return {
    view,
    query,
    loading,
    error,
    connect,
    open,
    refresh: () => open(query.value, true),
    dispose,
  };
}
