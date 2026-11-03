import type { CoreClient, ArtistReference } from './contracts.ts';
import type { AlbumCard, CatalogResource } from './catalog.ts';
import type { PlaySelection, Quality } from './player.ts';

export type AlbumCardAction =
  | 'next'
  | 'queue'
  | 'collect'
  | 'remove'
  | 'share'
  | 'copy'
  | 'download';
export interface AlbumCardRequest {
  album: AlbumCard;
  action: AlbumCardAction;
  anchor: HTMLElement | null;
}

/** Official AlbumCard keeps the primary artist first when present in the list. */
export function albumCredits(album: AlbumCard): ArtistReference[] {
  const primary = album.primaryArtist;
  if (!album.artists.length) return primary ? [primary] : [];
  const own = primary?.id ? album.artists.find((artist) => artist.id === primary.id) : undefined;
  return own ? [own, ...album.artists.filter((artist) => artist.id !== own.id)] : album.artists;
}

/** Official M103.c/formatDate defaults to local-calendar YYYY-MM-DD. */
export function albumCardDate(value: number | null): string {
  if (!value) return '';
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return '';
  return [date.getFullYear(), date.getMonth() + 1, date.getDate()]
    .map((part) => String(part).padStart(2, '0'))
    .join('-');
}

export async function catalogResourceSelection(
  core: Pick<CoreClient, 'catalogPage'>,
  epoch: string,
  resource: CatalogResource,
  quality: Quality,
): Promise<PlaySelection> {
  const result = await core.catalogPage(epoch, resource, true);
  if (result.epoch !== epoch) throw { code: 'stale', message: '账号已切换' };
  const view = result.value;
  if (
    !view ||
    view.query.kind !== resource.kind ||
    view.query.id !== resource.id ||
    (view.page.kind !== 'artist' && view.page.kind !== 'album') ||
    view.page.kind !== resource.kind
  )
    throw { code: 'unavailable', message: '暂时无法读取歌曲，请重试' };
  const index = view.page.tracks.findIndex((track) => !track.missing);
  if (index < 0) throw { code: 'unavailable', message: '暂无可播放歌曲' };
  return {
    kind: 'catalog',
    pages: [{ query: view.query, revision: view.revision }],
    index,
    quality,
  };
}
