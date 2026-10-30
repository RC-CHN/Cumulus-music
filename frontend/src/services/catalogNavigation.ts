import type { ArtistTab, CatalogQuery, CatalogResource } from './catalog.ts';

export type AlbumTab = 'songs' | 'comments' | 'details';
export interface CatalogLocation {
  resource: CatalogResource;
  catalogVisit?: number;
  artistTab?: ArtistTab;
  albumTab?: AlbumTab;
}
export type CatalogPanelKey = `artist:${ArtistTab}` | `album:${AlbumTab}`;
export interface CatalogPosition {
  inner: number;
  outer: number;
  focus?: string;
}

export function catalogVisitKey(location: CatalogLocation) {
  return JSON.stringify([location.resource.kind, location.resource.id, location.catalogVisit ?? 0]);
}

export function artistQuery(tab: ArtistTab, id: string): CatalogQuery | undefined {
  switch (tab) {
    case 'all':
      return { kind: 'artist_songs', id, offset: 0, limit: 20 };
    case 'albums':
      return { kind: 'artist_albums', id, offset: 0, limit: 20 };
    case 'videos':
      return { kind: 'artist_videos', id, cursor: '0', size: 20 };
    case 'intro':
      return { kind: 'artist_introduction', id };
    case 'similar':
      return { kind: 'similar_artists', id, offset: 0, limit: 20 };
    case 'concerts':
      return { kind: 'artist_concerts', id, offset: 0, limit: 20 };
  }
}
