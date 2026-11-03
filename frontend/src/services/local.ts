import type { TrackView } from './contracts';
export type LocalGroup = 'list' | 'artist' | 'album' | 'folder';
export type LocalSort = 'title' | 'artist' | 'album' | 'duration' | 'format' | 'folder';
export interface LocalQuery {
  keyword: string;
  sort: LocalSort;
  descending: boolean;
  group: LocalGroup;
  groupKey: string | null;
  includeMissing: boolean;
  offset: number;
  limit: number;
  revision: string | null;
}
export const defaultLocalQuery = (): LocalQuery => ({
  keyword: '',
  sort: 'title',
  descending: true,
  group: 'list',
  groupKey: null,
  includeMissing: true,
  offset: 0,
  limit: 200,
  revision: null,
});
/** Revision is an index precondition; every presentation field owns its rows. */
export function localQueryKey(query: LocalQuery): string {
  return JSON.stringify([
    query.keyword,
    query.sort,
    query.descending,
    query.group,
    query.groupKey,
    query.includeMissing,
    query.offset,
    query.limit,
  ]);
}
/** Scope an action to the named native group, independent of the visible page/selection. */
export function localGroupQuery(
  query: LocalQuery,
  page: LocalPage,
  key: string,
): LocalQuery | null {
  if (query.group === 'list' || !page.groups.some((group) => group.key === key)) return null;
  return { ...query, groupKey: key, offset: 0, revision: null };
}
export interface LocalTrack {
  track: TrackView;
  revision: string;
  fileName: string;
  folder: string;
  format: string;
  cue: boolean;
  encrypted: boolean;
  unreadable: boolean;
}
export interface LocalRoot {
  id: string;
  path: string;
  enabled: boolean;
  status: 'pending' | 'scanning' | 'ready' | 'error' | 'disabled';
  progress: {
    stage: 'discovering' | 'probing' | 'committing';
    total: number;
    processed: number;
  } | null;
  lastScanMs: number | null;
  updated: number;
  unchanged: number;
  missing: number;
  failures: number;
  rootMissing: boolean;
  error: string | null;
}
export interface LocalPage {
  revision: string;
  tracks: LocalTrack[];
  groups: { key: string; name: string; count: number }[];
  roots: LocalRoot[];
  total: number;
  playable: number;
  offset: number;
  more: boolean;
}
export type LocalCommand =
  | { kind: 'watch'; path: string }
  | { kind: 'enable' | 'disable'; id: string }
  | { kind: 'rescan' };
export interface LocalClient {
  localPage(query: LocalQuery): Promise<LocalPage>;
  localCommand(command: LocalCommand): Promise<void>;
  pickLocalDirectory(): Promise<string | null>;
  openLocalTrackDirectory(id: string, revision: string): Promise<void>;
}
