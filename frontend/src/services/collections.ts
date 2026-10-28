import type { AccountData } from './contracts';
import type { AlbumCard, ArtistCard } from './catalog';
export type CollectionScope = 'saved_albums' | 'purchased_albums' | 'followed_artists';
export type CollectionQuery =
  | { kind: 'saved_albums'; offset: number; limit: number }
  | { kind: 'purchased_albums' }
  | { kind: 'followed_artists'; cursor: string; size: number };
export interface FollowUser {
  id: string;
  name: string;
  signature: string;
  avatarUrl: string | null;
  unavailable: boolean;
}
export interface FollowCard {
  artist: ArtistCard | null;
  user: FollowUser | null;
}
export interface CollectionView {
  query: CollectionQuery;
  fetchedAt: number;
  cached: boolean;
  total: number | null;
  next: CollectionQuery | null;
  page: { kind: 'albums'; albums: AlbumCard[] } | { kind: 'follows'; entries: FollowCard[] };
}
export interface CollectionClient {
  collectionPage(
    epoch: string,
    query: CollectionQuery,
    refresh: boolean,
  ): Promise<AccountData<CollectionView | null>>;
}
export function collectionKey(query: CollectionQuery) {
  switch (query.kind) {
    case 'saved_albums':
      return JSON.stringify([query.kind, query.offset, query.limit]);
    case 'purchased_albums':
      return query.kind;
    case 'followed_artists':
      return JSON.stringify([query.kind, query.cursor, query.size]);
  }
}
export function firstCollectionQuery(scope: CollectionScope): CollectionQuery {
  return scope === 'saved_albums'
    ? { kind: scope, offset: 0, limit: 50 }
    : scope === 'followed_artists'
      ? { kind: scope, cursor: '0', size: 19 }
      : { kind: scope };
}
