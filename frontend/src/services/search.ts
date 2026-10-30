import type { AccountData } from './contracts';
export interface SearchWord {
  keyword: string;
  label: string;
  tag: string | null;
}
export interface SearchChart {
  id: string;
  name: string;
  items: SearchWord[];
}
export type SearchDiscoveryQuery =
  | { kind: 'defaults' | 'charts' }
  | { kind: 'recommendations'; lastQuery: string }
  | { kind: 'chart'; id: string }
  | { kind: 'suggestions'; keyword: string };
export type SearchDiscovery =
  | { kind: 'defaults'; words: SearchWord[]; refreshMs: number }
  | { kind: 'recommendations'; words: SearchWord[] }
  | { kind: 'charts'; charts: SearchChart[] }
  | { kind: 'chart'; id: string; items: SearchWord[] }
  | { kind: 'suggestions'; title: string; recommended: SearchWord[]; suggestions: SearchWord[] };
export type SearchHistoryCommand =
  | { kind: 'read' | 'clear' }
  | { kind: 'remember' | 'remove'; keyword: string };
export interface SearchClient {
  searchDiscovery(
    epoch: string,
    query: SearchDiscoveryQuery,
  ): Promise<AccountData<SearchDiscovery>>;
  searchHistory(epoch: string, command: SearchHistoryCommand): Promise<AccountData<string[]>>;
}
