import type { DownloadQuery, DownloadSort } from './downloads';

export type DownloadSection = 'song' | 'voice' | 'mv' | 'active';
/** Recovered default table bases and flex growth; the index column is fixed. */
export function downloadColumnWidths(width: number, section: DownloadSection): number[] {
  const base =
    section === 'active'
      ? [45, 180, 140]
      : section === 'voice'
        ? [45, 180, 90, 60]
        : [45, 180, 115, 90, 60];
  const growth =
    section === 'active'
      ? [0, 18, 14]
      : section === 'voice'
        ? [0, 360, 45, 30]
        : [0, 360, 230, 45, 30];
  const extra = Math.max(0, width - base.reduce((a, b) => a + b, 0));
  const total = growth.reduce((a, b) => a + b, 0);
  return base.map((value, i) => value + (extra * growth[i]!) / total);
}
export const downloadSections: { id: DownloadSection; label: string }[] = [
  { id: 'song', label: '已下载单曲' },
  { id: 'voice', label: '已下载声音' },
  { id: 'mv', label: '已下载MV' },
  { id: 'active', label: '正在下载' },
];
export function downloadSection(query: DownloadQuery): DownloadSection {
  return query.tab === 'active'
    ? 'active'
    : query.category === 'voice' || query.category === 'mv'
      ? query.category
      : 'song';
}
export function downloadSectionQuery(section: DownloadSection): Partial<DownloadQuery> {
  return {
    tab: section === 'active' ? 'active' : 'completed',
    category: section === 'active' ? 'all' : section,
    keyword: '',
    sort: 'completed',
    descending: section !== 'active',
    offset: 0,
  };
}
/** DownloadedTrackViewer title cycles name then artist; other columns are binary. */
export function nextDownloadSort(
  query: DownloadQuery,
  column: DownloadSort,
): Partial<DownloadQuery> {
  if (column === 'name') {
    if (query.sort === 'name')
      return query.descending
        ? { sort: 'artist', descending: false }
        : { sort: 'name', descending: true };
    if (query.sort === 'artist')
      return query.descending
        ? query.category === 'voice'
          ? { sort: 'completed', descending: true }
          : { sort: 'name', descending: false }
        : { sort: 'artist', descending: true };
  }
  return { sort: column, descending: query.sort === column ? !query.descending : false };
}
export function downloadDate(ms: number | null): string {
  if (ms === null) return '—';
  const date = new Date(ms);
  if (!Number.isFinite(date.getTime())) return '—';
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
