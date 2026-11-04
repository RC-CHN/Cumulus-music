import { computed, reactive, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  ProfileRelationCard,
  ProfileRelationClient,
  ProfileRelationKind,
  ProfileRelationTarget,
} from '../services/profileRelations';

interface List {
  items: ProfileRelationCard[];
  continuation: string | null;
  recordCount: number | null;
  loaded: boolean;
  busy: boolean;
  failed: boolean;
  stale: boolean;
  top: number;
  focus: number | null;
}
interface Visit {
  target: ProfileRelationTarget;
  kind: ProfileRelationKind;
  permission: 'ready' | 'checking' | 'failed';
  lists: Partial<Record<ProfileRelationKind, List>>;
}

export function useProfileRelations(core: ProfileRelationClient, expired: () => void) {
  const checking = ref(false);
  const entryNotice = ref('');
  const denied = ref('');
  const visit = shallowRef<Visit>();
  const cache = new Map<number, Visit>();
  const list = computed(() => visit.value?.lists[visit.value.kind]);
  let gateRevision = 0;
  let revision = 0;
  let epoch = '';

  function clearGate() {
    gateRevision++;
    checking.value = false;
    entryNotice.value = '';
  }

  async function check(owner: string, uid: string) {
    if (checking.value) return;
    const current = ++gateRevision;
    checking.value = true;
    entryNotice.value = '';
    try {
      const reply = await core.profileRelationAccess(owner, uid);
      if (current !== gateRevision) return;
      if (reply.epoch !== owner) throw new Error('profile relation account changed');
      if (reply.value.state === 'denied') {
        entryNotice.value = reply.value.message;
        return;
      }
      return reply.value;
    } catch (error) {
      if (current !== gateRevision) return;
      if (['session_expired', 'profile_changed'].includes(failure(error).code)) expired();
      else entryNotice.value = '无法确认关注与粉丝的查看权限，请重试';
    } finally {
      if (current === gateRevision) checking.value = false;
    }
  }

  function currentList(state: Visit, kind: ProfileRelationKind) {
    return (state.lists[kind] ??= {
      items: [],
      continuation: null,
      recordCount: null,
      loaded: false,
      busy: false,
      failed: false,
      stale: false,
      top: 0,
      focus: null,
    });
  }

  async function load() {
    const state = visit.value;
    const page = list.value;
    if (
      !state ||
      !page ||
      state.permission !== 'ready' ||
      page.busy ||
      (page.loaded && !page.continuation)
    )
      return;
    const ticket = ++revision;
    const owner = epoch;
    const kind = state.kind;
    const marker = page.continuation;
    page.busy = true;
    page.failed = false;
    page.stale = false;
    try {
      const reply = await core.profileRelationPage(owner, state.target.id, kind, marker);
      if (ticket !== revision) return;
      if (reply.epoch !== owner) throw new Error('profile relation account changed');
      if (reply.value.state === 'denied') {
        cache.delete(state.target.relationVisit);
        denied.value = reply.value.message;
        return;
      }
      page.items = [...page.items, ...reply.value.items];
      page.continuation = reply.value.continuation;
      page.recordCount = reply.value.recordCount ?? page.recordCount;
      page.loaded = true;
    } catch (error) {
      if (ticket !== revision) return;
      const issue = failure(error);
      if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
      else {
        page.failed = true;
        page.stale = issue.code === 'stale';
      }
    } finally {
      if (ticket === revision) page.busy = false;
    }
  }

  function retry() {
    const page = list.value;
    if (page?.stale) {
      page.items = [];
      page.continuation = null;
      page.recordCount = null;
      page.loaded = false;
      page.top = 0;
      page.focus = null;
    }
    return load();
  }

  async function recheck() {
    const state = visit.value;
    if (!state || state.target.isSelf) return;
    const current = ++revision;
    const owner = epoch;
    state.permission = 'checking';
    try {
      const reply = await core.profileRelationAccess(owner, state.target.id);
      if (current !== revision) return;
      if (reply.epoch !== owner) throw new Error('profile relation account changed');
      if (reply.value.state === 'denied') {
        cache.delete(state.target.relationVisit);
        denied.value = reply.value.message;
        return;
      }
      state.permission = 'ready';
    } catch (error) {
      if (current !== revision) return;
      if (['session_expired', 'profile_changed'].includes(failure(error).code)) expired();
      else state.permission = 'failed';
    }
  }

  function cancel() {
    revision++;
    const active = list.value;
    if (active) active.busy = false;
  }

  function enter(owner: string, target: ProfileRelationTarget | null) {
    cancel();
    denied.value = '';
    if (owner !== epoch) {
      epoch = owner;
      cache.clear();
      clearGate();
    }
    if (!owner || !target || target.owner !== owner) {
      visit.value = undefined;
      return;
    }
    const old = cache.get(target.relationVisit);
    const state =
      old ?? reactive<Visit>({ target, kind: target.mode, permission: 'ready', lists: {} });
    if (!old) {
      cache.set(target.relationVisit, state);
      if (cache.size > 8) cache.delete(cache.keys().next().value!);
    }
    visit.value = state;
    const page = currentList(state, state.kind);
    if (old && !state.target.isSelf && page.loaded) return recheck();
    if (!page.loaded && !page.failed) return load();
  }

  function select(kind: ProfileRelationKind) {
    const state = visit.value;
    if (!state || state.kind === kind || !state.target.isSelf || state.target.mode !== 'following')
      return;
    if (kind === 'followers') return;
    cancel();
    state.kind = kind;
    const page = currentList(state, kind);
    if (!page.loaded && !page.failed) void load();
  }

  function observeFollow(uid: string, followed: boolean, mutual: boolean) {
    const active = visit.value;
    let refreshActive = false;
    for (const state of cache.values()) {
      // These two collections changed membership. Do not guess ordering,
      // cursor or count by adding/removing a single client-side card.
      if (state.target.isSelf && state.lists.following) {
        refreshActive ||= state === active && state.kind === 'following';
        delete state.lists.following;
      }
      if (state.target.id === uid && state.lists.followers) {
        refreshActive ||= state === active && state.kind === 'followers';
        delete state.lists.followers;
      }
      for (const page of Object.values(state.lists)) {
        for (const item of page.items) {
          if (item.kind === 'user' && item.id === uid)
            item.relationship = followed ? (mutual ? 'mutual' : 'following') : null;
        }
      }
    }
    if (!active || !refreshActive) return;
    cancel();
    currentList(active, active.kind);
    if (active.target.isSelf) void load();
    else
      void recheck().then(() => {
        if (visit.value === active && active.permission === 'ready' && !denied.value) void load();
      });
  }

  function remember(top: number, focus?: number) {
    const page = list.value;
    if (page) {
      page.top = top;
      if (focus !== undefined) page.focus = focus;
    }
  }

  return {
    checking,
    entryNotice,
    denied,
    visit,
    list,
    enter,
    check,
    clearGate,
    recheck,
    retry,
    load,
    select,
    remember,
    observeFollow,
  };
}
