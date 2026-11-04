import type { AlbumCard } from './catalog';
import type { AccountData, TrackView } from './contracts';

export const latestSongRegions = [
  ['0', '全部'],
  ['7', '华语'],
  ['96', '欧美'],
  ['16', '韩国'],
  ['8', '日本'],
  ['536870911', '小语种'],
] as const;

export const latestAlbumRegions = [
  ['ALL', '全部'],
  ['ZH', '华语'],
  ['EA', '欧美'],
  ['KR', '韩国'],
  ['JP', '日本'],
  ['OTHER', '其他'],
] as const;

export type LatestSongArea = (typeof latestSongRegions)[number][0];
export type LatestAlbumArea = (typeof latestAlbumRegions)[number][0];
export type LatestReleaseTab = 'songs' | 'albums';
export interface LatestReleaseCursor {
  year: number;
  month: number;
  offset: number;
}
export type LatestReleaseQuery =
  | { kind: 'songs'; area: LatestSongArea }
  | {
      kind: 'albums';
      area: LatestAlbumArea;
      recommended: boolean;
      cursor: LatestReleaseCursor | null;
    };
export type LatestReleasePage =
  | { kind: 'songs'; revision: string; tracks: TrackView[]; playableIds: string[] }
  | {
      kind: 'albums';
      year: number;
      month: number;
      week: AlbumCard[];
      albums: AlbumCard[];
      next: LatestReleaseCursor | null;
    };
export interface LatestReleaseClient {
  latestReleases(epoch: string, query: LatestReleaseQuery): Promise<AccountData<LatestReleasePage>>;
}

export type LatestSongPage = Extract<LatestReleasePage, { kind: 'songs' }>;
export interface LatestSongRowSelection {
  area: LatestSongArea;
  revision: string;
  start_id: string;
}
export interface LatestAlbumGroup {
  key: string;
  title: string;
  albums: AlbumCard[];
}
export interface LatestReleasePosition {
  top: number;
  focus?: string;
}

export function latestCursorKey(cursor: LatestReleaseCursor | null) {
  return cursor ? `${cursor.year}:${cursor.month}:${cursor.offset}` : 'initial';
}

export function latestPlayable(page: LatestSongPage): TrackView[] {
  const eligible = new Set(page.playableIds);
  return page.tracks.filter((track) => !track.local && !track.missing && eligible.has(track.id));
}

export function appendLatestAlbums(
  groups: LatestAlbumGroup[],
  page: Extract<LatestReleasePage, { kind: 'albums' }>,
): LatestAlbumGroup[] {
  const next = groups.slice();
  function append(key: string, title: string, albums: AlbumCard[]) {
    if (!albums.length) return;
    const index = next.findIndex((group) => group.key === key);
    const old = index >= 0 ? next[index].albums : [];
    const seen = new Set(old.map((album) => album.id));
    const additions = albums.filter((album) => {
      if (seen.has(album.id)) return false;
      seen.add(album.id);
      return true;
    });
    if (!additions.length) return;
    const group = { key, title, albums: [...old, ...additions] };
    if (index >= 0) next[index] = group;
    else if (key === 'week') next.unshift(group);
    else next.push(group);
  }
  append('week', '本周新碟', page.week);
  append(`${page.year}:${page.month}`, `${page.year}年${page.month}月`, page.albums);
  return next;
}
