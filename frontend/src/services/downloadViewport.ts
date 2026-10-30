import type { DownloadQuery } from './downloads.ts';

/** Last download view in this account, kept in memory for navigation only. */
export interface DownloadViewport {
  epoch: string;
  query: string;
  anchor: string;
  index: number;
  offset: number;
  focus?: { task: string; action?: string };
  growth: number[];
}
export function downloadViewportKey(query: DownloadQuery) {
  return JSON.stringify([
    query.tab,
    query.category,
    query.keyword,
    query.sort,
    query.descending,
    query.offset,
  ]);
}
