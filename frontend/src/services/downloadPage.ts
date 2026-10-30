import { tableColumnWidths, type TableColumn } from './tableColumns.ts';
import type { DownloadQuery, DownloadSort } from './downloads';

export type DownloadSection = 'song' | 'voice' | 'mv' | 'active';
const column = (width: number, grow: number): TableColumn => ({ width, grow, resize: grow > 0 });
const songColumns = [
  column(45, 0),
  column(180, 360),
  column(115, 230),
  column(90, 45),
  column(60, 30),
];
const voiceColumns = [column(45, 0), column(180, 360), column(90, 45), column(60, 30)];
const activeColumns = [column(45, 0), column(180, 18), column(140, 14)];
export function downloadTableColumns(section: DownloadSection): readonly TableColumn[] {
  return section === 'active' ? activeColumns : section === 'voice' ? voiceColumns : songColumns;
}
/** Recovered default table bases and flex growth; the index column is fixed. */
export function downloadColumnWidths(width: number, section: DownloadSection): number[] {
  return tableColumnWidths(width, downloadTableColumns(section));
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
          ? { sort: 'none', descending: false }
          : { sort: 'name', descending: false }
        : { sort: 'artist', descending: true };
  }
  if (query.category === 'voice' && query.sort === column && query.descending)
    return { sort: 'none', descending: false };
  return { sort: column, descending: query.sort === column ? !query.descending : false };
}
export function downloadDate(ms: number | null): string {
  if (ms === null) return '—';
  const date = new Date(ms);
  if (!Number.isFinite(date.getTime())) return '—';
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
