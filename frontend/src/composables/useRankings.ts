import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import {
  rankingQueryKey,
  rankingSelection,
  type RankingClient,
  type RankingQuery,
  type RankingView,
  type RankingSelection,
  type RankingUiState,
} from '../services/rankings.ts';
import { useCore } from '../services/core.ts';

export function useRankings(core: RankingClient = useCore(), expired = () => {}) {
  const view = shallowRef<RankingView>();
  const query = shallowRef<RankingQuery>({ kind: 'home' });
  const loading = ref(false);
  const error = ref('');
  const selection = ref<RankingSelection>(rankingSelection({ kind: 'home' }));
  const restoration = shallowRef<RankingUiState>();
  let epoch: string | undefined;
  let serial = 0;
  let disposed = false;
  let visit = 0;
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    view.value = undefined;
    loading.value = false;
    error.value = '';
    visit++;
    selection.value = rankingSelection(query.value);
    restoration.value = undefined;
  }
  function destination(target: RankingQuery, value: RankingSelection): RankingUiState {
    return {
      ...rankingSelection(target, value),
      epoch: epoch ?? '',
      queryKey: rankingQueryKey(target),
      scrollTop: 0,
      focus: null,
    };
  }
  function checkpoint(viewport: Pick<RankingUiState, 'scrollTop' | 'focus'>): RankingUiState {
    return { ...destination(query.value, selection.value), ...viewport };
  }
  function enter(target: RankingQuery, saved?: RankingUiState) {
    visit++;
    const valid = !!saved && saved.epoch === epoch && saved.queryKey === rankingQueryKey(target);
    selection.value = rankingSelection(target, valid ? saved : undefined);
    restoration.value = valid ? saved : undefined;
    return open(target);
  }
  function captureVisit() {
    const captured = visit;
    return () => !disposed && captured === visit;
  }
  function leave() {
    visit++;
    serial++;
    loading.value = false;
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
    selection,
    restoration,
    connect,
    enter,
    destination,
    checkpoint,
    captureVisit,
    leave,
    open,
    refresh: () => open(query.value, true),
    dispose,
  };
}
