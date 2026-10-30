import type { AccountData } from './contracts';
import type { TrackView, PlaylistView } from './contracts';
import type { AlbumCard, ArtistCard, CatalogVideoCard } from './catalog';
import type { EpisodeView, PodcastCard } from './podcast';
import type { FollowUser } from './collections';
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
export interface SearchResultQuery {
  keyword: string;
  category: SearchCategory;
  offset: number;
  limit: number;
  correct: boolean;
}
export type SearchItem =
  | { kind: 'song'; track: TrackView; excerpt: string[]; excerptTruncated?: boolean }
  | { kind: 'playlist'; playlist: PlaylistView }
  | { kind: 'artist'; artist: ArtistCard }
  | { kind: 'album'; album: AlbumCard }
  | { kind: 'voice'; voice: EpisodeView }
  | { kind: 'podcast'; podcast: PodcastCard }
  | { kind: 'mv'; mv: CatalogVideoCard }
  | { kind: 'user'; user: FollowUser }
  | { kind: 'related'; keyword: string };
export interface SearchSection {
  id: string;
  title: string;
  category: SearchCategory | null;
  items: SearchItem[];
}
export interface SearchResultView {
  query: SearchResultQuery;
  sections: SearchSection[];
  total: number | null;
  more: boolean;
  nextOffset: number;
  correction: string | null;
  unavailableCount: number;
  notice: string | null;
}
export interface SearchResultsClient {
  searchResults(epoch: string, query: SearchResultQuery): Promise<AccountData<SearchResultView>>;
}
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
