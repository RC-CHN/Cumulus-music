import { ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { searchCardKey } from '../services/searchCards.ts';
import {
  createSearchViewportMemory,
  type SearchPosition,
  type SearchViewportMemory,
} from '../services/searchViewport.ts';
import type {
  SearchResultQuery,
  SearchResultView,
  SearchResultsClient,
} from '../services/search.ts';

export function useSearchResults(
  core: SearchResultsClient,
  expired: () => void,
  memory: SearchViewportMemory = createSearchViewportMemory(),
) {
  const view = shallowRef<SearchResultView>(),
    loading = ref(false),
    error = ref(''),
    loadingMore = ref(false),
    moreError = ref(''),
    position = shallowRef<SearchPosition>({ top: 0, tracks: 0 });
  let activeKey = '';
  let generation = 0,
    account: AccountView | undefined;
  function connect(next: AccountView) {
    generation++;
    account = next;
    memory.connect(next);
    view.value = undefined;
    error.value = '';
    loading.value = false;
    loadingMore.value = false;
    moreError.value = '';
  }
  function savePosition(next: SearchPosition) {
    position.value = next;
    if (view.value)
      memory.set(
        activeKey,
        { view: view.value, position: next, moreError: moreError.value },
        account?.epoch ?? '',
      );
  }
  function save() {
    if (view.value)
      memory.set(
        activeKey,
        {
          view: view.value,
          position: position.value,
          moreError: moreError.value,
        },
        account?.epoch ?? '',
      );
  }
  async function open(query: SearchResultQuery, reuse = false, session = 0) {
    if (account?.status !== 'authenticated') return;
    const epoch = account.epoch,
      current = ++generation;
    activeKey = memory.key(query, session);
    const cached = reuse ? memory.get(activeKey, epoch) : undefined;
    loadingMore.value = false;
    moreError.value = cached?.moreError ?? '';
    position.value = cached?.position ?? { top: 0, tracks: 0 };
    if (cached) {
      view.value = cached.view;
      error.value = '';
      loading.value = false;
      return;
    }
    view.value = undefined;
    loading.value = true;
    error.value = '';
    try {
      const result = await core.searchResults(epoch, query);
      if (current === generation && account?.epoch === epoch && result.epoch === epoch) {
        view.value = result.value;
        save();
      }
    } catch (e) {
      if (current !== generation) return;
      const issue = failure(e);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
        memory.clear();
        expired();
      }
    } finally {
      if (current === generation) loading.value = false;
    }
  }
  async function more() {
    const previous = view.value;
    if (
      !previous?.more ||
      loading.value ||
      loadingMore.value ||
      account?.status !== 'authenticated'
    )
      return;
    const current = generation,
      epoch = account.epoch;
    loadingMore.value = true;
    moreError.value = '';
    try {
      if (previous.nextOffset <= previous.query.offset || previous.nextOffset > 10000)
        throw { code: 'unavailable', message: '暂时无法继续加载，请重新搜索' };
      const result = await core.searchResults(epoch, {
        ...previous.query,
        offset: previous.nextOffset,
      });
      if (current !== generation || account?.epoch !== epoch || result.epoch !== epoch) return;
      const next = result.value;
      if (next.nextOffset <= previous.nextOffset && next.more)
        throw { code: 'unavailable', message: '搜索结果未更新，请重试' };
      let added = 0;
      const sections = previous.sections.map((s) => ({ ...s, items: [...s.items] }));
      for (const incoming of next.sections) {
        let section = sections.find((s) => s.id === incoming.id);
        if (!section) {
          section = { ...incoming, items: [] };
          sections.push(section);
        }
        const seen = new Set(section.items.map(searchCardKey));
        for (const item of incoming.items) {
          const key = searchCardKey(item);
          if (!seen.has(key)) {
            section.items.push(item);
            seen.add(key);
            added++;
          }
        }
      }
      view.value = {
        ...next,
        query: previous.query,
        sections,
        more: next.more && added > 0,
        unavailableCount: previous.unavailableCount + next.unavailableCount,
        notice: !added && next.more ? '没有更多可显示的结果，已保留当前内容' : next.notice,
      };
      save();
    } catch (e) {
      if (current !== generation) return;
      const issue = failure(e);
      moreError.value = issue.message;
      save();
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
        memory.clear();
        expired();
      }
    } finally {
      if (current === generation) loadingMore.value = false;
    }
  }
  function dispose() {
    generation++;
    account = undefined;
    view.value = undefined;
    loading.value = false;
    loadingMore.value = false;
  }
  return {
    view,
    loading,
    error,
    loadingMore,
    moreError,
    position,
    connect,
    open,
    more,
    savePosition,
    dispose,
  };
}
