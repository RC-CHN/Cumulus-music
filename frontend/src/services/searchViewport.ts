import type { AccountView } from './contracts.ts';
import type { SearchCategory, SearchResultQuery, SearchResultView } from './search.ts';

export interface SearchPosition {
  top: number;
  tracks: number;
  focus?: string;
  focusAction?: string;
  expandedLyrics?: string[];
}
interface Snapshot {
  view: SearchResultView;
  position: SearchPosition;
  moreError: string;
}
/** Application-owned, account-isolated browsing memory; never persisted to disk. */
export function createSearchViewportMemory(capacity = 16) {
  let owner = '';
  const entries = new Map<string, Snapshot>();
  const searchTabs = new Map<number, readonly SearchCategory[]>();
  function clear() {
    entries.clear();
    searchTabs.clear();
  }
  return {
    connect(account: AccountView) {
      const next = account.status === 'authenticated' ? account.epoch : '';
      if (!next || owner !== next) clear();
      owner = next;
    },
    key(query: SearchResultQuery, session: number) {
      return JSON.stringify([
        session,
        query.keyword,
        query.category,
        query.offset,
        query.limit,
        query.correct,
        query.channel,
      ]);
    },
    get(key: string, epoch: string) {
      if (owner !== epoch) return;
      const value = entries.get(key);
      if (value) {
        entries.delete(key);
        entries.set(key, value);
      }
      return value;
    },
    set(key: string, value: Snapshot, epoch: string) {
      if (!owner || owner !== epoch) return;
      entries.delete(key);
      entries.set(key, value);
      while (entries.size > capacity) entries.delete(entries.keys().next().value!);
    },
    getTabs(session: number, epoch: string) {
      if (!owner || owner !== epoch) return;
      const value = searchTabs.get(session);
      if (value) {
        searchTabs.delete(session);
        searchTabs.set(session, value);
      }
      return value;
    },
    setTabs(session: number, tabs: readonly SearchCategory[], epoch: string) {
      if (session <= 0 || !owner || owner !== epoch) return;
      searchTabs.delete(session);
      searchTabs.set(session, [...tabs]);
      while (searchTabs.size > capacity) searchTabs.delete(searchTabs.keys().next().value!);
    },
    clear,
  };
}
export type SearchViewportMemory = ReturnType<typeof createSearchViewportMemory>;
