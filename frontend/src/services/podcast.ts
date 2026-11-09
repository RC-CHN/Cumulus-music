import type { AccountData } from './contracts';
export type PodcastRecommendationAction = 'overlay' | 'play' | 'next';

export function podcastPublicLink(id: string): string | null {
  return /^[1-9]\d{0,19}$/.test(id) ? `https://music.163.com/djradio?id=${id}` : null;
}
export interface PodcastProgramShare {
  id: string;
  name: string;
  creator: string;
}
export function podcastProgramPublicLink(id: string): string | null {
  return id.length <= 20 && /^[1-9]/.test(id) && !/\D/.test(id)
    ? `https://music.163.com/dj?id=${id}`
    : null;
}
export interface PodcastProgramTarget {
  radioId: string;
  ascending: boolean;
  revision: string;
  voiceId: string;
}
export interface PodcastRecentProgramTarget {
  revision: string;
  voiceId: string;
}
export interface PodcastRecentProgramMenu {
  commentCount: number | null;
  downloadComplete: boolean | null;
}
export interface PodcastProgramMenuClient {
  podcastRecentProgramMenu(
    epoch: string,
    target: PodcastRecentProgramTarget,
  ): Promise<AccountData<PodcastRecentProgramMenu>>;
  podcastRecentProgram(
    epoch: string,
    target: PodcastRecentProgramTarget,
  ): Promise<AccountData<EpisodeView>>;
  podcastOpenRecentProgramDownloadDirectory(
    epoch: string,
    target: PodcastRecentProgramTarget,
  ): Promise<void>;
  podcastProgramCommentCount(
    epoch: string,
    target: PodcastProgramTarget,
  ): Promise<AccountData<number | null>>;
  podcastOpenProgramDownloadDirectory(epoch: string, target: PodcastProgramTarget): Promise<void>;
}
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
  newEpisodeCount?: number;
  subscribed: boolean | null;
  feeType: number;
  unavailable: boolean;
}
export interface EpisodeView {
  browsePlaybackReceipt?: string;
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
  subscribedBadgeReceipt?: string;
  episodes: EpisodeView[];
  total: number;
  offset: number;
  remoteTotal: number;
  radio: PodcastCard | null;
  likedIds: string[];
  resumeEnabled: boolean | null;
  audiobookSkipEnabled?: boolean | null;
  operations: PodcastMutation[];
}
export type PodcastCommand =
  | { kind: 'change_search_subscription'; id: string; expected: boolean }
  | { kind: 'change_vinyl_subscription'; play_id: string; expected: boolean }
  | { kind: 'collect_recommendation'; id: string; receipt: string; revision: string }
  | { kind: 'change'; action: PodcastAction; id: string; desired: boolean }
  | { kind: 'reconcile'; action: PodcastAction; id: string };
export interface PodcastState {
  likedIds: string[] | null;
  operations: PodcastMutation[];
}
export type PodcastRadioOrder =
  | { kind: 'read'; id: string }
  | {
      kind: 'set';
      scope: Extract<PodcastScope, { kind: 'radio' }>;
      revision: string;
      ascending: boolean;
    };
export interface PodcastSubscriptionRecommendation {
  id: string;
  name: string;
  coverUrl: string | null;
  playCount: number | null;
  unavailable: boolean;
  ownedByOther?: boolean;
}
export interface PodcastSubscriptionRecommendations {
  receipt: string;
  cards: PodcastSubscriptionRecommendation[];
}
export interface PodcastClient {
  podcastSubscriptionRecommendations(
    epoch: string,
  ): Promise<AccountData<PodcastSubscriptionRecommendations>>;
  podcastState(epoch: string, refresh: boolean): Promise<AccountData<PodcastState>>;
  podcastRadioOrder(epoch: string, query: PodcastRadioOrder): Promise<AccountData<boolean | null>>;
  podcastPage(epoch: string, query: PodcastQuery, refresh: boolean): Promise<PodcastPage>;
  podcastClearSubscribedBadge(epoch: string, receipt: string, id: string): Promise<void>;
  podcastCommand(epoch: string, command: PodcastCommand): Promise<AccountData<PodcastMutation>>;
}
export interface PodcastRecentEpisode extends EpisodeView {
  listenerCount: number | null;
  listenLocationMs: number | null;
  listened: boolean;
}
export interface PodcastRecentPage {
  revision: string;
  episodes: PodcastRecentEpisode[];
  hasMore: boolean;
  capped: boolean;
  newVoiceCount: number;
}
export type PodcastRecentQuery = { kind: 'open' | 'refresh' } | { kind: 'next'; revision: string };
export interface PodcastRecentClient {
  podcastRecent(epoch: string, query: PodcastRecentQuery): Promise<AccountData<PodcastRecentPage>>;
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
export interface PodcastCatalogueCard extends PodcastCard {
  score: string | null;
}
export type PodcastCatalogue =
  | { kind: 'categories'; categories: PodcastCategory[] }
  | { kind: 'radios'; cards: PodcastCatalogueCard[]; continuation: string | null };
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
  | { kind: 'top_config'; layout: PodcastTopLayout; sceneFmAllowed: boolean }
  | { kind: 'book_groups'; groups: PodcastRankMenu[] }
  | { kind: 'menu'; tabs: PodcastRankTab[] }
  | { kind: 'entries'; rows: PodcastRankRow[] };
export interface PodcastRankingClient {
  podcastRankings(epoch: string, query: PodcastRankQuery): Promise<AccountData<PodcastRanks>>;
}
