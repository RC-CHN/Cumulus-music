import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type {
  PodcastClient,
  PodcastSearchClient,
  PodcastSearchFollow,
} from '../services/podcast.ts';

export function useSearchPodcastFollow(
  core: PodcastSearchClient & Pick<PodcastClient, 'podcastCommand'>,
  account: () => AccountView,
  active: () => boolean,
  expired: () => void,
) {
  const subscribed = shallowRef<Set<string>>();
  const pending = shallowRef(new Set<string>());
  const writing = shallowRef<{ owner: string }>();
  const status = ref('');
  let epoch = '';
  let generation = 0;
  let read = 0;

  function displayedOwner() {
    const current = account();
    return active() && current.status === 'authenticated' ? current.epoch : '';
  }
  function valid(owner: string, visit: number) {
    return !!owner && epoch === owner && displayedOwner() === owner && generation === visit;
  }
  function issue(error: unknown, announce: boolean) {
    const problem = failure(error);
    if (problem.code === 'session_expired' || problem.code === 'profile_changed') expired();
    else if (announce)
      status.value = problem.code === 'mutation_uncertain' ? '收藏操作待核对' : problem.message;
  }
  async function refresh(confirmed?: { id: string; subscribed: boolean }) {
    const owner = epoch,
      visit = generation,
      sequence = ++read;
    if (!owner) return;
    try {
      const response = await core.podcastSearchMembership(owner);
      if (!valid(owner, visit) || sequence !== read || response.epoch !== owner) return;
      const ids =
        response.value.subscribedIds === null ? undefined : new Set(response.value.subscribedIds);
      // The mutation's verified detail can be fresher than the account list.
      // Never use it to manufacture a complete list from an unknown reply.
      if (ids && confirmed) {
        if (confirmed.subscribed) ids.add(confirmed.id);
        else ids.delete(confirmed.id);
      }
      subscribed.value = ids;
      pending.value = new Set(response.value.pendingIds);
    } catch (error) {
      if (!valid(owner, visit) || sequence !== read) return;
      subscribed.value = undefined;
      issue(error, false);
    }
  }
  watch(
    displayedOwner,
    (owner) => {
      epoch = owner;
      generation++;
      subscribed.value = undefined;
      pending.value = new Set();
      status.value = '';
      if (owner) void refresh();
    },
    { immediate: true },
  );
  onScopeDispose(() => {
    generation++;
    epoch = '';
  });
  function control(id: string): PodcastSearchFollow | undefined {
    if (!epoch || displayedOwner() !== epoch) return;
    const busy = writing.value?.owner === epoch;
    if (pending.value.has(id)) return { kind: 'reconcile', busy };
    if (subscribed.value) return { kind: 'toggle', subscribed: subscribed.value.has(id), busy };
  }
  async function activate(id: string, expected: PodcastSearchFollow) {
    const owner = epoch,
      visit = generation;
    const current = control(id);
    if (!owner || !current || current.busy || expected.kind !== current.kind) return;
    if (
      current.kind === 'toggle' &&
      (expected.kind !== 'toggle' || expected.subscribed !== current.subscribed)
    )
      return;
    const write = { owner };
    writing.value = write;
    status.value = '';
    // An earlier passive read cannot overwrite the completed action.
    read++;
    let confirmed: { id: string; subscribed: boolean } | undefined;
    try {
      const response = await core.podcastCommand(
        owner,
        current.kind === 'toggle'
          ? { kind: 'change_search_subscription', id, expected: current.subscribed }
          : { kind: 'reconcile', action: 'subscribe', id },
      );
      if (!valid(owner, visit) || response.epoch !== owner) return;
      const action = response.value;
      if (['confirmed', 'unchanged'].includes(action.phase) && action.observed !== null)
        confirmed = { id, subscribed: action.observed };
      status.value = ['confirmed', 'unchanged'].includes(action.phase)
        ? action.observed
          ? '已收藏播客'
          : '已取消收藏播客'
        : '收藏操作待核对';
    } catch (error) {
      if (valid(owner, visit)) issue(error, true);
    } finally {
      if (writing.value === write) {
        if (epoch === owner && displayedOwner() === owner) await refresh(confirmed);
        if (valid(owner, visit) && pending.value.has(id)) status.value = '收藏操作待核对';
        if (writing.value === write) writing.value = undefined;
      }
    }
  }
  return { control, activate, status };
}
