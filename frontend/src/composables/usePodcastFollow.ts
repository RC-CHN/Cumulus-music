import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import type {
  PodcastClient,
  PodcastMutation,
  PodcastSearchClient,
  PodcastSearchFollow,
} from '../services/podcast.ts';

export function usePodcastFollow(
  core: PodcastSearchClient & Pick<PodcastClient, 'podcastCommand'>,
  account: () => AccountView,
  active: () => boolean,
  expired: () => void,
  playback?: {
    scope: () => string;
    receipt?: () => string | undefined;
    action: (
      owner: string,
      id: string,
      current: PodcastSearchFollow,
    ) => (() => Promise<AccountData<PodcastMutation>>) | undefined;
  },
) {
  const subscribed = shallowRef<Set<string>>();
  const pending = shallowRef(new Set<string>());
  const writing = shallowRef<{ owner: string }>();
  const refreshing = ref(false);
  const notice = shallowRef<{ scope: string; text: string }>();
  const scope = () => playback?.scope() ?? '';
  const status = computed(() => {
    const current = notice.value;
    return current && current.scope === scope() ? current.text : '';
  });
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
      notice.value = {
        scope: scope(),
        text: ['mutation_uncertain', 'write_uncertain'].includes(problem.code)
          ? '收藏操作待核对'
          : problem.message,
      };
  }
  async function refresh(
    confirmed?: { id: string; subscribed: boolean },
    ownsConfirmation = () => true,
  ) {
    const owner = epoch,
      visit = generation,
      sequence = ++read;
    if (!valid(owner, visit)) return;
    refreshing.value = true;
    try {
      const response = await core.podcastSearchMembership(owner);
      if (!valid(owner, visit) || sequence !== read || response.epoch !== owner) return;
      const ids =
        response.value.subscribedIds === null ? undefined : new Set(response.value.subscribedIds);
      // The mutation's verified detail can be fresher than the account list.
      // Never use it to manufacture a complete list from an unknown reply.
      if (ids && confirmed && ownsConfirmation()) {
        if (confirmed.subscribed) ids.add(confirmed.id);
        else ids.delete(confirmed.id);
      }
      subscribed.value = ids;
      pending.value = new Set(response.value.pendingIds);
    } catch (error) {
      if (!valid(owner, visit) || sequence !== read) return;
      subscribed.value = undefined;
      issue(error, false);
    } finally {
      if (valid(owner, visit) && sequence === read) refreshing.value = false;
    }
  }
  watch(
    displayedOwner,
    (owner) => {
      epoch = owner;
      generation++;
      subscribed.value = undefined;
      pending.value = new Set();
      notice.value = undefined;
      refreshing.value = false;
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
    const origin = scope();
    const receipt = playback?.receipt?.();
    const ownsAction = () =>
      valid(owner, visit) &&
      (!receipt || (playback?.receipt?.() === receipt && scope() === origin));
    const request = playback
      ? playback.action(owner, id, current)
      : () =>
          core.podcastCommand(
            owner,
            current.kind === 'toggle'
              ? { kind: 'change_search_subscription', id, expected: current.subscribed }
              : { kind: 'reconcile', action: 'subscribe', id },
          );
    if (!request || !ownsAction()) return;
    const announce = (text: string) => {
      if (scope() === origin) notice.value = { scope: origin, text };
    };
    const write = { owner };
    writing.value = write;
    notice.value = undefined;
    // An earlier passive read cannot overwrite the completed action.
    read++;
    let confirmed: { id: string; subscribed: boolean } | undefined;
    try {
      const response = await request();
      if (!ownsAction() || response.epoch !== owner) return;
      const action = response.value;
      if (action.id !== id || action.action !== 'subscribe') return;
      if (['confirmed', 'unchanged'].includes(action.phase) && action.observed !== null)
        confirmed = { id, subscribed: action.observed };
      announce(
        ['confirmed', 'unchanged'].includes(action.phase)
          ? action.observed
            ? '已收藏播客'
            : '已取消收藏播客'
          : action.phase === 'rejected'
            ? '收藏操作未成功'
            : '收藏操作待核对',
      );
    } catch (error) {
      if (ownsAction()) issue(error, scope() === origin);
    } finally {
      if (writing.value === write) {
        if (epoch === owner && displayedOwner() === owner) await refresh(confirmed, ownsAction);
        if (ownsAction() && pending.value.has(id)) announce('收藏操作待核对');
        if (writing.value === write) writing.value = undefined;
      }
    }
  }
  return { control, activate, status, refreshing, retry: () => refresh() };
}
