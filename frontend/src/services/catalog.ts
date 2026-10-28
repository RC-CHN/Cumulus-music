import type { AccountData, ArtistReference, TrackView } from './contracts';

export type CatalogQuery =
  | { kind: 'artist' | 'artist_dynamic' | 'artist_introduction' | 'album'; id: string }
  | {
      kind: 'artist_songs' | 'artist_albums' | 'similar_artists' | 'artist_concerts';
      id: string;
      offset: number;
      limit: number;
    }
  | { kind: 'artist_videos'; id: string; cursor: string; size: number };

export interface ArtistCard {
  id: string;
  name: string;
  aliases: string[];
  coverUrl: string | null;
  description: string;
  songCount: number;
  albumCount: number;
  mvCount: number | null;
}
export interface AlbumCard {
  id: string;
  name: string;
  aliases: string[];
  translations: string[];
  description: string;
  artists: ArtistReference[];
  coverUrl: string | null;
  trackCount: number;
  publishedAt: number | null;
  subscribed: boolean | null;
  explicit: boolean;
}
export interface CatalogVideoCard {
  id: string;
  kind: 'mv' | 'video' | 'unknown';
  title: string;
  description: string;
  artists: ArtistReference[];
  coverUrl: string | null;
  durationMs: number;
  playCount: number;
  mvId: string | null;
}
export interface ConcertCard {
  id: string;
  title: string;
  coverUrl: string | null;
  start: number | string | null;
  end: number | string | null;
  address: string;
  price: string;
}
export interface AlbumReadState {
  source: 'v3' | 'v4';
  privilegesRefreshed: boolean;
  digitalAlbumTag: boolean | null;
}
export type CatalogPage =
  | { kind: 'artist'; artist: ArtistCard; tracks: TrackView[]; more: boolean }
  | { kind: 'artist_dynamic'; followed: boolean; onlineCount: number; videoCount: number | null }
  | { kind: 'artist_songs'; offset: number; total: number; more: boolean; tracks: TrackView[] }
  | { kind: 'artist_albums'; offset: number; total: number; more: boolean; albums: AlbumCard[] }
  | {
      kind: 'artist_introduction';
      briefDescription: string;
      sections: { title: string; text: string }[];
    }
  | { kind: 'similar_artists'; offset: number; more: boolean; artists: ArtistCard[] }
  | {
      kind: 'artist_videos';
      nextCursor: string;
      more: boolean;
      size: number;
      videos: CatalogVideoCard[];
    }
  | {
      kind: 'artist_concerts';
      offset: number;
      total: number;
      more: boolean;
      concerts: ConcertCard[];
    }
  | { kind: 'album'; album: AlbumCard; tracks: TrackView[]; read: AlbumReadState | null };

export interface CatalogView {
  query: CatalogQuery;
  fetchedAt: number;
  cached: boolean;
  revision: string;
  page: CatalogPage;
}
export interface CatalogClient {
  catalogPage(
    epoch: string,
    query: CatalogQuery,
    refresh: boolean,
  ): Promise<AccountData<CatalogView | null>>;
}
