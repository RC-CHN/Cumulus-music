import type { AccountData } from './contracts';
import type { TrackView, PlaylistView } from './contracts';
import type { AlbumCard, ArtistCard, CatalogVideoCard } from './catalog';
import type { EpisodeView, PodcastCard } from './podcast';
import type { FollowUser } from './collections';
import type { SearchTextPart } from './searchMatch';
export const searchCategories = [
  { key: 'all', title: '综合', unit: '' },
  { key: 'songs', title: '单曲', unit: '首单曲' },
  { key: 'playlists', title: '歌单', unit: '个歌单' },
  { key: 'artists', title: '歌手', unit: '位歌手' },
  { key: 'voices', title: '声音', unit: '条声音' },
  { key: 'podcasts', title: '播客', unit: '个播客' },
  { key: 'lyrics', title: '歌词', unit: '篇歌词' },
  { key: 'albums', title: '专辑', unit: '张专辑' },
  { key: 'mvs', title: 'MV', unit: '支 MV' },
  { key: 'users', title: '用户', unit: '位用户' },
] as const;
export type SearchCategory = (typeof searchCategories)[number]['key'];
export function displayedSearchCategories(
  ordered: readonly SearchCategory[] | undefined,
  current: SearchCategory,
) {
  if (!ordered) return searchCategories;
  // The comprehensive route stays accessible even if the service omits it.
  const keys = new Set<SearchCategory>(ordered.includes('all') ? ordered : ['all', ...ordered]);
  keys.add(current);
  return [...keys].flatMap((key) => searchCategories.filter((tab) => tab.key === key));
}
export type SearchChannel = 'hotquery' | 'typing' | 'defaultquery' | 'history' | 'rec_query';
export interface SearchResultQuery {
  keyword: string;
  category: SearchCategory;
  offset: number;
  limit: number;
  correct: boolean;
  channel?: SearchChannel;
}
export type SearchTrackHint =
  | { kind: 'lyric'; parts: { text: string; highlight: boolean }[] }
  | { kind: 'alias'; text: string };
export type SearchQualityBadge = 'jymaster' | 'sky' | 'jyeffect' | 'hires' | 'sq';
export type SearchReasonTarget =
  | { kind: 'recommendation'; reasonId: string; songId: string; name: string }
  | { kind: 'city'; chartCode: string; targetId: string; targetType: string };
export type SearchReasonTag = { label: string; target?: SearchReasonTarget };
export type SearchReasonPage = { title: string; playCount: number | null; tracks: TrackView[] };
export type PodcastSearchTable = {
  creatorId?: string;
  programCount?: number;
  playCount?: number;
  score?: string;
  recommendation?: string;
  subscriptionEligible?: boolean;
};
export type SearchItem =
  | {
      kind: 'song';
      track: TrackView;
      excerpt: string[];
      queueDropReceipt?: string;
      excerptTruncated?: boolean;
      mvId?: string;
      replacementMv?: boolean;
      vipBadge?: boolean;
      limitedFree?: boolean;
      qualityBadge?: SearchQualityBadge;
      hint?: SearchTrackHint;
      explicit?: boolean;
      originalRecording?: boolean;
      reasons?: SearchReasonTag[];
    }
  | { kind: 'playlist'; playlist: PlaylistView; recommendation?: string; knownEmpty?: boolean }
  | { kind: 'artist'; artist: ArtistCard }
  | { kind: 'album'; album: AlbumCard }
  | { kind: 'voice'; voice: EpisodeView; listenerCount?: number; creatorId?: string }
  | { kind: 'podcast'; podcast: PodcastCard; table?: PodcastSearchTable; playbackReceipt?: string }
  | { kind: 'mv'; mv: CatalogVideoCard }
  | { kind: 'user'; user: FollowUser }
  | { kind: 'related'; keyword: string };
export interface SearchSection {
  id: string;
  title: string;
  category: SearchCategory | null;
  items: SearchItem[];
  /** Only set after a successful native privilege refresh of this block. */
  playAllIds?: string[];
}
export function comprehensivePlayAllTracks(section: SearchSection): TrackView[] {
  const rows = section.items.flatMap((item) => (item.kind === 'song' ? [item.track] : []));
  const ids = section.playAllIds;
  if (!ids) return rows;
  const selected = rows.slice(0, ids.length);
  return selected.length === ids.length && selected.every((track, index) => track.id === ids[index])
    ? selected
    : [];
}
export type SearchHotPlaybackTarget = {
  kind: 'song' | 'voice';
  id: string;
};
export type SearchHotTarget =
  | SearchHotPlaybackTarget
  | ({ kind: 'vertical_zone' } & import('./verticalZone').VerticalZoneLocation)
  | {
      kind: 'album' | 'artist' | 'playlist' | 'podcast' | 'mv';
      id: string;
    };
export interface SearchHotExplanation {
  title?: string;
  text?: string;
  pictureUrl?: string;
  pictureCornerMarker?: string;
  hotValue?: string;
  action?: { label: string; target: SearchHotTarget };
  playTarget?: SearchHotPlaybackTarget;
}
export type SearchPreviewLayout = 'compact_grid' | 'carousel' | 'compact_carousel';

export interface SearchResultView {
  query: SearchResultQuery;
  tabs?: SearchCategory[];
  hotExplanation?: SearchHotExplanation;
  previewLayout?: SearchPreviewLayout;
  sections: SearchSection[];
  privilegeReceipt?: string;
  presentationReceipt?: string;
  presentationRevision?: string;
  total: number | null;
  more: boolean;
  nextOffset: number;
  correction: string | null;
  correctionParts?: { text: string; highlight: boolean }[];
  unavailableCount: number;
  emptyTitle?: string;
  notice: string | null;
}
export function searchExplanation(
  view: SearchResultView | undefined,
  category: SearchCategory,
  hasResults: boolean,
) {
  const hot = category === 'all' ? view?.hotExplanation : undefined;
  const title = hot?.title?.trim();
  const text = hot?.text?.trim();
  const emptyTitle = hasResults ? undefined : title || view?.emptyTitle;
  const notice = view?.notice !== emptyTitle ? view?.notice : undefined;
  const lines = [...new Set([title, text])].filter(
    (line): line is string => !!line && line !== emptyTitle && line !== notice,
  );
  return { emptyTitle, notice, lines };
}
export interface SearchPrivilegeObservation {
  epoch: string;
  revision: string;
}
export interface SearchPrivilegePatch {
  receipt: string;
  section: string;
  playAllIds: string[];
  rows: {
    id: string;
    coverUrl: string | null;
    mvId: string | null;
    replacementMv: boolean;
    vipBadge: boolean;
    limitedFree: boolean;
    qualityBadge: SearchQualityBadge | null;
  }[];
}
export interface SearchResultsClient {
  searchPresentation?(
    epoch: string,
    receipt: string,
    revision: string,
  ): Promise<AccountData<SearchPresentationPatch>>;
  searchRefreshPrivileges?(
    epoch: string,
    receipt: string,
    observation?: string,
  ): Promise<AccountData<SearchPrivilegePatch>>;
  onSearchPrivilegeChanged?(
    handler: (event: SearchPrivilegeObservation) => void,
  ): Promise<() => void>;

  searchReasonPage(
    epoch: string,
    target: SearchReasonTarget,
  ): Promise<AccountData<SearchReasonPage>>;
  searchResults(
    epoch: string,
    query: SearchResultQuery,
    defaultReceipt?: string,
  ): Promise<AccountData<SearchResultView>>;
}
export interface SearchPresentationPatch {
  receipt: string;
  revision: string;
  previewLayout: SearchPreviewLayout | null;
  sections: {
    id: string;
    items: {
      index: number;
      trackId: string;
      hint: SearchTrackHint | null;
      reasons: SearchReasonTag[];
    }[];
  }[];
}
export interface SearchWord {
  keyword: string;
  label: string;
  tag: string | null;
  channel?: SearchChannel;
  radioId?: string;
  defaultReceipt?: string;
}
export interface SearchSuggestionWord extends SearchWord {
  labelParts: SearchTextPart[];
}
export type SearchChartDestination = 'podcasts' | 'audiobooks';
export interface SearchChart {
  id: string;
  name: string;
  items: SearchWord[];
  needPlay?: boolean;
  audio?: boolean;
  more?: { label: string; target: SearchChartDestination };
  playKeywords?: string[] | null;
}
export type SearchDiscoveryQuery =
  | { kind: 'defaults' | 'charts' }
  | { kind: 'recommendations'; lastQuery: string }
  | { kind: 'chart'; id: string }
  | { kind: 'chart_songs'; keywords: string[] }
  | { kind: 'suggestions'; keyword: string };
export type SearchDiscovery =
  | { kind: 'defaults'; words: SearchWord[]; refreshMs: number }
  | { kind: 'recommendations'; words: SearchWord[] }
  | { kind: 'charts'; charts: SearchChart[] }
  | { kind: 'chart'; id: string; items: SearchWord[]; playKeywords?: string[] | null }
  | { kind: 'chart_songs'; ids: string[] }
  | {
      kind: 'suggestions';
      title: string;
      recommended: SearchSuggestionWord[];
      suggestions: SearchSuggestionWord[];
    };
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
