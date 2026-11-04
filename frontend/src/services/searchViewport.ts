import type { LyricAnchor } from './lyricWindow.ts';
import type { AccountView } from './contracts.ts';
import type { SearchCategory, SearchResultQuery, SearchResultView } from './search.ts';

export interface SearchPosition {
  top: number;
  tracks: number;
  focus?: string;
  focusSection?: string;
  focusAction?: string;
  expandedLyrics?: string[];
  lyricAnchor?: LyricAnchor;
  previewPositions?: Record<string, number>;
}
interface Snapshot {
  view: SearchResultView;
  position: SearchPosition;
  moreError: string;
  manualMore: boolean;
}
export interface SearchCorrectionReminder {
  rewrite: string;
  parts: NonNullable<SearchResultView['correctionParts']>;
}
/** Application-owned, account-isolated browsing memory; never persisted to disk. */
export function createSearchViewportMemory(capacity = 16) {
  let owner = '';
  const entries = new Map<string, Snapshot>();
  const searchTabs = new Map<number, readonly SearchCategory[]>();
  const reminders = new Map<string, SearchCorrectionReminder>();
  function clear() {
    entries.clear();
    searchTabs.clear();
    reminders.clear();
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
    getReminder(session: number, keyword: string, epoch: string) {
      if (session <= 0 || !owner || owner !== epoch) return;
      const key = JSON.stringify([session, keyword]);
      const value = reminders.get(key);
      if (value) {
        reminders.delete(key);
        reminders.set(key, value);
      }
      return value;
    },
    setReminder(session: number, keyword: string, epoch: string, value?: SearchCorrectionReminder) {
      if (session <= 0 || !owner || owner !== epoch) return;
      const key = JSON.stringify([session, keyword]);
      reminders.delete(key);
      if (value) reminders.set(key, value);
      while (reminders.size > capacity) reminders.delete(reminders.keys().next().value!);
    },
    clear,
  };
}
export type SearchViewportMemory = ReturnType<typeof createSearchViewportMemory>;
