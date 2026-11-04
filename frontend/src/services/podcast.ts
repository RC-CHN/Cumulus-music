import type { AccountData } from './contracts';
/** Extract only an official radio identifier; this never opens the pasted URL. */
export function podcastReference(input: string): string | null {
  const value = input.trim();
  if (/^[1-9]\d{0,19}$/.test(value)) return value;
  if (value.length > 2048) return null;
  try {
    const url = new URL(value);
    if (
      !['http:', 'https:'].includes(url.protocol) ||
      url.hostname !== 'music.163.com' ||
      url.username ||
      url.password ||
      url.port
    )
      return null;
    const route = url.hash.startsWith('#/') ? new URL(url.hash.slice(1), url.origin) : url;
    if (route.pathname !== '/djradio' || route.searchParams.getAll('id').length !== 1) return null;
    const id = route.searchParams.get('id') ?? '';
    return /^[1-9]\d{0,19}$/.test(id) ? id : null;
  } catch {
    return null;
  }
}
export type PodcastScope =
  | { kind: 'podcasts' | 'audiobooks' | 'created' | 'purchased' | 'liked' }
  | { kind: 'radio'; id: string; ascending: boolean };
export type PodcastAction = 'subscribe' | 'like' | 'resume';
export interface PodcastCard {
  id: string;
  name: string;
  coverUrl: string | null;
  creator: string;
  description: string;
  programCount: number;
  subscribed: boolean | null;
  feeType: number;
  unavailable: boolean;
}
export interface EpisodeView {
  id: string;
  name: string;
  coverUrl: string | null;
  creator: string;
  radioId: string | null;
  radioName: string;
  durationMs: number;
  feeType: number;
  liked: boolean;
  unavailable: boolean;
}
export interface PodcastQuery {
  scope: PodcastScope;
  keyword: string;
  offset: number;
  limit: number;
  revision: string | null;
}
export interface PodcastMutation {
  action: PodcastAction;
  id: string;
  desired: boolean;
  observed: boolean | null;
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'rejected' | 'unchanged';
  acknowledged: boolean;
}
export interface PodcastPage {
  epoch: string;
  scope: PodcastScope;
  revision: string;
  cards: PodcastCard[];
  episodes: EpisodeView[];
  total: number;
  offset: number;
  remoteTotal: number;
  radio: PodcastCard | null;
  likedIds: string[];
  resumeEnabled: boolean | null;
  operations: PodcastMutation[];
}
export type PodcastCommand =
  | { kind: 'change_search_subscription'; id: string; expected: boolean }
  | { kind: 'change'; action: PodcastAction; id: string; desired: boolean }
  | { kind: 'reconcile'; action: PodcastAction; id: string };
export interface PodcastState {
  likedIds: string[] | null;
  operations: PodcastMutation[];
}
export interface PodcastClient {
  podcastState(epoch: string, refresh: boolean): Promise<AccountData<PodcastState>>;
  podcastPage(epoch: string, query: PodcastQuery, refresh: boolean): Promise<PodcastPage>;
  podcastCommand(epoch: string, command: PodcastCommand): Promise<AccountData<PodcastMutation>>;
}
export interface PodcastSearchMembership {
  subscribedIds: string[] | null;
  pendingIds: string[];
}
export interface PodcastSearchClient {
  podcastSearchMembership(epoch: string): Promise<AccountData<PodcastSearchMembership>>;
}
export type PodcastSearchFollow =
  | { kind: 'toggle'; subscribed: boolean; busy: boolean }
  | { kind: 'reconcile'; busy: boolean };

export const AUDIOBOOK_CATEGORY = '10001';
export type PodcastCatalogueQuery =
  | { kind: 'categories' }
  | { kind: 'radios'; parentId: string; subId: string; continuation?: string };
export interface PodcastSubcategory {
  id: string;
  label: string;
}
export interface PodcastCategory extends PodcastSubcategory {
  children: PodcastSubcategory[];
}
export type PodcastCatalogue =
  | { kind: 'categories'; categories: PodcastCategory[] }
  | { kind: 'radios'; cards: PodcastCard[]; continuation: string | null };
export interface PodcastCatalogueClient {
  podcastCatalogue(
    epoch: string,
    query: PodcastCatalogueQuery,
  ): Promise<AccountData<PodcastCatalogue>>;
}

export type PodcastHomeBlock =
  | { kind: 'radios'; title: string | null; cards: PodcastCard[] }
  | { kind: 'voices'; title: string | null; groups: EpisodeView[][] };
export interface PodcastHomePage {
  recommendationEnabled: boolean;
  blocks: PodcastHomeBlock[];
  continuation: string | null;
}
export interface PodcastHomeClient {
  podcastHome(epoch: string, continuation?: string): Promise<AccountData<PodcastHomePage>>;
}

export type PodcastRankGroup =
  | 'voice'
  | 'audio'
  | 'talent'
  | 'overall_charts'
  | 'free_charts'
  | 'new_charts'
  | 'vip_charts';
export interface PodcastRankTab {
  id: string;
  name: string;
  chartType: string;
  showScore: boolean;
  showRankOffset: boolean;
}
export interface PodcastRankTarget {
  group: PodcastRankGroup;
  id: string;
  chartType: string;
}
export type PodcastTopLayout = 'classic' | 'updated';
export interface PodcastRankMenu {
  group: PodcastRankGroup;
  name: string;
  tabs: PodcastRankTab[];
}
export type PodcastRankResource =
  | { kind: 'radio'; radio: PodcastCard }
  | { kind: 'voice'; voice: EpisodeView }
  | { kind: 'host'; id: string; name: string; avatarUrl: string | null };
export interface PodcastRankRow {
  rank: number | null;
  lastRank: number | null;
  score: string | null;
  resource: PodcastRankResource;
}
export type PodcastRankQuery =
  | { kind: 'top_config' }
  | { kind: 'book_groups' }
  | { kind: 'menu'; group: PodcastRankGroup }
  | ({ kind: 'entries'; preview?: boolean } & PodcastRankTarget);
export type PodcastRanks =
  | { kind: 'top_config'; layout: PodcastTopLayout }
  | { kind: 'book_groups'; groups: PodcastRankMenu[] }
  | { kind: 'menu'; tabs: PodcastRankTab[] }
  | { kind: 'entries'; rows: PodcastRankRow[] };
export interface PodcastRankingClient {
  podcastRankings(epoch: string, query: PodcastRankQuery): Promise<AccountData<PodcastRanks>>;
}
