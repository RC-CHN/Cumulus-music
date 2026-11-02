import type { AccountData, TrackView } from './contracts';
export type RankingQuery =
  | { kind: 'home' }
  | { kind: 'album_sales'; year: number }
  | { kind: 'album_year'; year: number }
  | { kind: 'song_sales' }
  | { kind: 'album'; id: string };
export type RankingPeriod = 'daily' | 'week' | 'year' | 'total';
export interface RankingSelection {
  period: RankingPeriod;
  singles: boolean;
}
export interface RankingUiState extends RankingSelection {
  epoch: string;
  queryKey: string;
  scrollTop: number;
  focus: {
    kind: 'card' | 'album' | 'song';
    id: string;
    action?: 'open' | 'play' | 'like' | 'comments';
  } | null;
}
export const rankingQueryKey = (query: RankingQuery): string =>
  JSON.stringify([
    query.kind,
    query.kind === 'album' ? query.id : 'year' in query ? query.year : null,
  ]);
export function rankingSelection(query: RankingQuery, state?: RankingSelection): RankingSelection {
  return {
    period: query.kind === 'album_year' ? 'year' : (state?.period ?? 'daily'),
    singles: state?.singles ?? false,
  };
}
export interface RankingPreview {
  id: string;
  rank: number | null;
  lastRank: number | null;
  name: string;
  artist: string;
  coverUrl: string | null;
  isAlbum: boolean;
}
export interface RankingCard {
  id: string;
  target: 'playlist' | 'album_sales' | 'song_sales';
  name: string;
  nameShowStyle: string;
  updateFrequency: string;
  canPlay: boolean;
  coverUrl: string | null;
  textCover: boolean;
  tracks: RankingPreview[];
}
export interface RankingGroup {
  name: string;
  displayType: string;
  cards: RankingCard[];
}
export interface AlbumSale {
  id: string;
  name: string;
  artist: string;
  coverUrl: string | null;
  rank: number | null;
  rankIncr: number | null;
  newest: boolean;
  saleNum: number | null;
  certification: string | null;
}
export interface SalesBoard {
  period: string;
  year: number | null;
  singles: boolean;
  products: AlbumSale[];
}
export interface SongSale {
  track: TrackView;
  quantity: number | null;
  indicator: number | null;
  rankIncr: number | null;
  newest: boolean;
}
export type RankingPage =
  | { kind: 'home'; groups: RankingGroup[] }
  | { kind: 'album_sales'; year: number; boards: SalesBoard[] }
  | { kind: 'song_sales'; generatedAt: number | null; songs: SongSale[] }
  | {
      kind: 'album';
      id: string;
      name: string;
      description: string;
      artists: string[];
      coverUrl: string | null;
      revision: string;
      tracks: TrackView[];
    };
export interface RankingView {
  fetchedAt: number;
  cached: boolean;
  fresh: boolean;
  page: RankingPage;
}
export interface RankingClient {
  rankingPage(
    epoch: string,
    query: RankingQuery,
    refresh: boolean,
  ): Promise<AccountData<RankingView>>;
}
export function movement(row: RankingPreview): string {
  if (row.lastRank === 0 && !row.isAlbum) return '新';
  if (row.rank === row.lastRank) return '—';
  return row.rank !== null && row.lastRank !== null && row.rank < row.lastRank ? '↑' : '↓';
}
