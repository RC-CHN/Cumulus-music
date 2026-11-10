import type { LyricAnchor } from './lyricWindow.ts';
import type { AccountView } from './contracts.ts';
import type { SearchCategory, SearchResultQuery, SearchResultView } from './search.ts';
import type { PcPresentationView } from './pcPresentation.ts';

export function presentationRevision(value: unknown): bigint | undefined {
  // Native public revisions are canonical decimal u64 counters, not credentials.
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]{0,19})$/.test(value)) return;
  const revision = BigInt(value);
  return revision <= 18_446_744_073_709_551_615n ? revision : undefined;
}

export function withoutSearchFooters(view: SearchResultView): SearchResultView {
  if (view.query.category !== 'all') return view;
  let changed = false;
  const sections = view.sections.map((section) => ({
    ...section,
    items: section.items.map((item) => {
      if (item.kind !== 'song' || (!item.hint && !item.reasons?.length)) return item;
      changed = true;
      return { ...item, hint: undefined, reasons: [] };
    }),
  }));
  // A locally redacted view is no longer the projection the native revision
  // describes. Keep its receipt so that a delayed matching event can restore
  // the authorized fields, even if that revision already arrived in the search.
  return changed ? { ...view, presentationRevision: undefined, sections } : view;
}

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
  receipt?: string;
}
/** Application-owned, account-isolated browsing memory; never persisted to disk. */
export function createSearchViewportMemory(capacity = 16) {
  let owner = '';
  const entries = new Map<string, Snapshot>();
  const searchTabs = new Map<number, readonly SearchCategory[]>();
  const reminders = new Map<string, SearchCorrectionReminder>();
  let presentation: PcPresentationView | undefined;
  let presentationOrder: bigint | undefined;
  const presentationListeners = new Set<(view: PcPresentationView) => void>();
  function clear() {
    entries.clear();
    searchTabs.clear();
    reminders.clear();
  }
  return {
    presentation(epoch: string) {
      return owner === epoch ? presentation : undefined;
    },
    observePresentation(next: PcPresentationView): 'changed' | 'unchanged' | 'rejected' {
      const revision = presentationRevision(next.revision);
      if (
        !owner ||
        owner !== next.epoch ||
        revision === undefined ||
        (presentationOrder !== undefined && revision < presentationOrder)
      )
        return 'rejected';
      if (presentation && revision === presentationOrder)
        return next.searchFooter === presentation.searchFooter &&
          next.searchExpanded === presentation.searchExpanded &&
          next.searchDense === presentation.searchDense
          ? 'unchanged'
          : 'rejected';
      presentation = { ...next };
      presentationOrder = revision;
      if (!next.searchFooter)
        for (const [key, snapshot] of entries)
          entries.set(key, { ...snapshot, view: withoutSearchFooters(snapshot.view) });
      for (const listener of presentationListeners) listener(presentation);
      return 'changed';
    },
    onPresentation(listener: (view: PcPresentationView) => void) {
      presentationListeners.add(listener);
      return () => presentationListeners.delete(listener);
    },
    connect(account: AccountView) {
      const next = account.status === 'authenticated' ? account.epoch : '';
      if (!next || owner !== next) {
        clear();
        presentation = undefined;
        presentationOrder = undefined;
      }
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
      entries.set(key, {
        ...value,
        view:
          presentation && !presentation.searchFooter
            ? withoutSearchFooters(value.view)
            : value.view,
      });
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
