import type { AccountView } from './contracts.ts';
import type { SearchResultQuery, SearchResultView } from './search.ts';

export interface SearchPosition {
  top: number;
  tracks: number;
  focus?: string;
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
  return {
    connect(account: AccountView) {
      const next = account.status === 'authenticated' ? account.epoch : '';
      if (!next || owner !== next) entries.clear();
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
    clear() {
      entries.clear();
    },
  };
}
export type SearchViewportMemory = ReturnType<typeof createSearchViewportMemory>;
