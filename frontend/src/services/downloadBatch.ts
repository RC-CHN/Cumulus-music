import type { DownloadQuery, DownloadTask } from './downloads';
export type BatchCategory = 'song' | 'voice';
export type BatchOrder = { column: 'album' | 'completed' | 'size'; descending: boolean };
export function batchDownloadQuery(parent: DownloadQuery, category: BatchCategory): DownloadQuery {
  return { ...parent, tab: 'completed', category, keyword: '', offset: 0, limit: 1000000 };
}
export function orderedDownloads(tasks: DownloadTask[], order?: BatchOrder) {
  if (!order) return tasks;
  return [...tasks].sort(
    (a, b) =>
      (order.descending ? -1 : 1) *
      (order.column === 'album'
        ? a.track.album.localeCompare(b.track.album)
        : order.column === 'completed'
          ? (a.completedMs ?? 0) - (b.completedMs ?? 0)
          : (a.total ?? 0) - (b.total ?? 0)),
  );
}
export function nextBatchOrder(previous: BatchOrder | undefined, column: BatchOrder['column']) {
  return previous?.column !== column
    ? { column, descending: false }
    : previous.descending
      ? undefined
      : { column, descending: true };
}
