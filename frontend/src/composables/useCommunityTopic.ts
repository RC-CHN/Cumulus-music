import { reactive, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { CommunityCard } from '../services/community';
import type {
  CommunityTopic,
  CommunityTopicAction,
  CommunityTopicClient,
} from '../services/communityTopic';

export type TopicGroup = 'hot' | 'all';
type Opener = { group: TopicGroup; key: string };
function slice() {
  return {
    cards: [] as CommunityCard[],
    pending: false,
    loaded: false,
    error: '',
    last: null as CommunityTopicAction | null,
  };
}
function visit(name: string) {
  return {
    name,
    meta: undefined as CommunityTopic | null | undefined,
    pending: false,
    error: '',
    unavailable: false,
    refreshing: false,
    hasNew: false,
    hot: slice(),
    all: slice(),
    continuation: null as string | null,
    scrollTop: 0,
    group: 0,
    opener: null as Opener | null,
  };
}
export type TopicVisit = ReturnType<typeof visit>;

export function useCommunityTopic(core: CommunityTopicClient, expired: () => void) {
  const active = shallowRef<TopicVisit>();
  const visits = new Map<number, TopicVisit>();
  let owner = '';
  let generation = 0;

  function report(error: unknown) {
    const code = failure(error).code;
    if (code === 'session_expired' || code === 'profile_changed') expired();
  }
  function allowed(state: TopicVisit, version: number) {
    return generation === version && [...visits.values()].includes(state);
  }
  function clear(state: TopicVisit) {
    state.unavailable = true;
    state.hot.cards = [];
    state.all.cards = [];
    state.continuation = null;
  }
  async function read(state: TopicVisit, group: TopicGroup, action: CommunityTopicAction) {
    const lane = state[group];
    if (!owner || active.value !== state || !state.meta || state.unavailable || lane.pending)
      return;
    const version = generation;
    lane.last = action;
    lane.pending = true;
    lane.error = '';
    try {
      const reply = await core.communityTopicSlice(owner, state.meta.handle, action);
      if (!allowed(state, version) || reply.epoch !== owner || state.unavailable) return;
      const value = reply.value;
      if (value.unavailable) {
        clear(state);
        return;
      }
      const patches = new Map(value.cards.map((card) => [card.key, card]));
      const merged = new Map((value.replace ? [] : lane.cards).map((card) => [card.key, card]));
      for (const [key, card] of patches) merged.set(key, card);
      for (const key of value.retired) merged.delete(key);
      lane.cards = [...merged.values()];
      lane.loaded = true;
      // The same native event may be in both lanes. Apply native anonymity
      // and locator revocations to both displays without reordering either.
      const other = state[group === 'hot' ? 'all' : 'hot'];
      other.cards = other.cards.map((card) => patches.get(card.key) ?? card);
      if (group === 'all') {
        state.continuation = value.continuation;
        if (value.replace) state.group = 0;
        else if (value.retired.length)
          state.group = Math.max(0, state.group - Math.floor(value.retired.length / 20));
      }
    } catch (error) {
      if (!allowed(state, version) || state.unavailable) return;
      report(error);
      lane.error = group === 'hot' ? '暂时无法读取热门笔记。' : '暂时无法读取全部笔记。';
    } finally {
      if (allowed(state, version)) lane.pending = false;
    }
  }
  function ensure(state: TopicVisit) {
    if (active.value !== state || !state.meta || state.unavailable) return;
    if (!state.hot.loaded && !state.hot.error) void read(state, 'hot', { kind: 'hot' });
    if (!state.all.loaded && !state.all.error) void read(state, 'all', { kind: 'first' });
  }
  async function open(state: TopicVisit) {
    if (!owner || state.pending) return;
    const version = generation;
    state.pending = true;
    state.error = '';
    try {
      const reply = await core.communityTopic(owner, state.name);
      if (!allowed(state, version) || reply.epoch !== owner) return;
      state.meta = reply.value;
      if (!reply.value) clear(state);
      else ensure(state);
    } catch (error) {
      if (!allowed(state, version)) return;
      report(error);
      state.error = '暂时无法读取话题。';
    } finally {
      if (allowed(state, version)) state.pending = false;
    }
  }
  function enter(epoch: string, target: { name: string; visit: number } | null) {
    if (epoch !== owner) {
      owner = epoch;
      generation++;
      visits.clear();
    }
    active.value = undefined;
    if (!owner || !target) return;
    let state = visits.get(target.visit);
    if (!state) {
      state = reactive(visit(target.name));
      visits.set(target.visit, state);
      if (visits.size > 3) visits.delete(visits.keys().next().value!);
    }
    active.value = state;
    if (state.meta === undefined && !state.error) void open(state);
    else ensure(state);
  }
  async function refresh() {
    const state = active.value;
    if (!state || state.refreshing || state.all.pending || state.hot.pending) return;
    state.refreshing = true;
    try {
      await read(state, 'all', { kind: 'refresh' });
      if (!state.unavailable) await read(state, 'hot', { kind: 'hot' });
      if (!state.all.error && !state.hot.error) state.hasNew = false;
    } finally {
      state.refreshing = false;
    }
  }
  async function notice(state: TopicVisit, current: () => boolean) {
    if (!owner || !state.meta || state.unavailable || active.value !== state || !current()) return;
    const version = generation;
    try {
      const reply = await core.communityTopicNotice(owner, state.meta.handle);
      if (allowed(state, version) && active.value === state && current() && reply.epoch === owner)
        state.hasNew = reply.value;
    } catch (error) {
      if (allowed(state, version) && active.value === state && current()) report(error);
    }
  }
  function retry(group?: TopicGroup) {
    const state = active.value;
    if (!state) return;
    if (!group) return open(state);
    const action = state[group].last;
    if (action) return read(state, group, action);
  }
  function next() {
    const state = active.value;
    if (state?.continuation && !state.all.error && !state.refreshing)
      return read(state, 'all', { kind: 'next', continuation: state.continuation });
  }
  return { active, enter, retry, next, refresh, notice };
}
