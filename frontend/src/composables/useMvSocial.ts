import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { MvClient, MvDetail, MvLikeOperation, MvRecommendations } from '../services/mv.ts';

export function useMvSocial(core: MvClient, expired: () => void) {
  const liked = ref<boolean | null>(null),
    count = ref(0),
    pending = shallowRef<MvLikeOperation>();
  const busy = ref(false),
    error = ref(''),
    recommendations = shallowRef<MvRecommendations>();
  const recommendationLoading = ref(false),
    recommendationError = ref('');
  let epoch = '',
    id = '',
    generation = 0,
    writes = 0,
    reading = 0;
  const valid = (g: number, e: string, resource: string) =>
    g === generation && e === epoch && resource === id && !!e;
  function issue(value: unknown): string {
    const f = failure(value);
    if (['session_expired', 'profile_changed'].includes(f.code)) expired();
    return f.message;
  }
  function reset() {
    generation++;
    writes++;
    reading++;
    id = '';
    liked.value = null;
    count.value = 0;
    pending.value = undefined;
    busy.value = false;
    error.value = '';
    recommendations.value = undefined;
    recommendationLoading.value = false;
    recommendationError.value = '';
  }
  function connect(account: AccountView) {
    reset();
    epoch = account.status === 'authenticated' ? account.epoch : '';
  }
  function dispose() {
    reset();
    epoch = '';
  }
  function activate(resource: string) {
    reset();
    id = resource;
    void loadRecommendations();
    return { generation, writes };
  }
  function acceptDetail(value: MvDetail, ticket: { generation: number; writes: number }) {
    if (ticket.generation !== generation || ticket.writes !== writes || value.card.id !== id)
      return;
    liked.value = value.liked;
    count.value = value.likeCount;
    pending.value = value.likeOperation ?? undefined;
  }
  async function loadRecommendations() {
    const g = generation,
      e = epoch,
      resource = id,
      request = ++reading;
    if (!e || !resource) return;
    recommendationLoading.value = true;
    recommendationError.value = '';
    try {
      const value = await core.mvRecommendations(e, resource, true);
      if (
        valid(g, e, resource) &&
        request === reading &&
        value.epoch === e &&
        value.value.id === resource
      )
        recommendations.value = value.value;
    } catch (value) {
      if (valid(g, e, resource) && request === reading) {
        recommendationError.value = issue(value);
        try {
          const cached = await core.mvRecommendations(e, resource, false);
          if (
            valid(g, e, resource) &&
            request === reading &&
            cached.epoch === e &&
            cached.value.id === resource
          )
            recommendations.value = cached.value;
        } catch {
          /* Preserve the explicit refresh error. */
        }
      }
    } finally {
      if (valid(g, e, resource) && request === reading) recommendationLoading.value = false;
    }
  }
  async function refreshLike() {
    const g = generation,
      e = epoch,
      resource = id,
      write = writes;
    if (!e || !resource) return;
    const value = await core.mvDetail(e, resource);
    if (valid(g, e, resource) && value.epoch === e)
      acceptDetail(value.value, { generation: g, writes: write });
  }
  async function like(desired: boolean, reconcile = false) {
    if (!epoch || !id || busy.value || (!reconcile && (pending.value || liked.value === null)))
      return;
    const g = generation,
      e = epoch,
      resource = id;
    writes++;
    busy.value = true;
    error.value = '';
    try {
      const result = await (reconcile
        ? core.mvLikeReconcile(e, resource)
        : core.mvLike(e, resource, desired));
      if (!valid(g, e, resource) || result.epoch !== e || result.value.id !== resource) return;
      const value = result.value;
      if (value.observed !== null) liked.value = value.observed;
      if (value.count !== null) count.value = value.count;
      pending.value = ['confirmed', 'unchanged', 'rejected'].includes(value.phase)
        ? undefined
        : value;
    } catch (value) {
      if (valid(g, e, resource)) {
        error.value = issue(value);
        try {
          await refreshLike();
        } catch {
          if (valid(g, e, resource))
            pending.value = {
              id: resource,
              desired,
              observed: null,
              count: null,
              phase: 'uncertain',
            };
        }
      }
    } finally {
      if (valid(g, e, resource)) {
        busy.value = false;
        writes++;
      }
    }
  }
  async function refresh() {
    if (busy.value) return;
    const g = generation,
      e = epoch,
      resource = id;
    try {
      await refreshLike();
      if (valid(g, e, resource)) error.value = '';
    } catch (value) {
      if (valid(g, e, resource)) error.value = issue(value);
    }
  }
  return {
    liked,
    count,
    pending,
    busy,
    error,
    recommendations,
    recommendationLoading,
    recommendationError,
    connect,
    dispose,
    activate,
    acceptDetail,
    like,
    refresh,
    loadRecommendations,
  };
}
