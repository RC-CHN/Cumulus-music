import type { CatalogQuery, CatalogView } from './catalog.ts';

export function nextCatalogQuery(view: CatalogView): CatalogQuery | null {
  const { query, page } = view;
  if (!('more' in page) || !page.more || page.kind !== query.kind) return null;
  if (query.kind === 'artist_videos' && page.kind === 'artist_videos') {
    if (page.nextCursor.replace(/^0+(?=\d)/, '') === query.cursor.replace(/^0+(?=\d)/, ''))
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
