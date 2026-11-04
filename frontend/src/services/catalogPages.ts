import type { CatalogQuery, CatalogVideoCard, CatalogView } from './catalog.ts';

export function artistHasMvs(count: number | null | undefined): boolean {
  return typeof count === 'number' && Number.isSafeInteger(count) && count > 0;
}

export type ArtistVideoSlot = { slot: string; video: CatalogVideoCard };

/** M2999 replaces [cursor-size, cursor) in the artist's existing rows. */
export function artistVideoSlots(pages: CatalogView[]): ArtistVideoSlot[] {
  const records = new Map<bigint, CatalogVideoCard>();
  for (const { page } of pages) {
    if (
      page.kind !== 'artist_videos' ||
      !/^\d{1,128}$/.test(page.nextCursor) ||
      !Number.isSafeInteger(page.size) ||
      page.size < page.videos.length ||
      page.size > 100
    )
      continue;
    const start = BigInt(page.nextCursor) - BigInt(page.size);
    for (let i = 0; i < page.size; i++) {
      const slot = start + BigInt(i);
      if (slot < 0) continue;
      const video = page.videos[i];
      if (video) records.set(slot, video);
      else records.delete(slot);
    }
  }
  return [...records]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([slot, video]) => ({ slot: slot.toString(), video }));
}

export function nextCatalogQuery(view: CatalogView): CatalogQuery | null {
  const { query, page } = view;
  if (!('more' in page) || !page.more || page.kind !== query.kind) return null;
  if (query.kind === 'artist_videos' && page.kind === 'artist_videos') {
    if (
      !/^\d{1,128}$/.test(page.nextCursor) ||
      !/^\d{1,128}$/.test(query.cursor) ||
      BigInt(page.nextCursor) <= BigInt(query.cursor)
    )
      return null;
    return { ...query, cursor: page.nextCursor };
  }
  if ('offset' in query && query.offset + query.limit <= 1000000)
    return { ...query, offset: query.offset + query.limit };
  return null;
}
export function catalogPageKey(query: CatalogQuery): string {
  return JSON.stringify([
    query.kind,
    query.id,
    'offset' in query ? query.offset : null,
    'limit' in query ? query.limit : null,
    'cursor' in query ? query.cursor : null,
    'size' in query ? query.size : null,
  ]);
}
