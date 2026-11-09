import type { AccountData, PlaylistView, TrackView } from './contracts';
import type { PlaylistPhase } from './playlists';
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
export const LOCAL_BATCH_PAGE_SIZE = 100;
export const LOCAL_BATCH_MAX_SELECTION = 50_000;
/** The batch route shows flat local tracks even when entered from a group. */
export function localBatchQuery(query: LocalQuery): LocalQuery {
  return {
    ...query,
    group: 'list',
    groupKey: null,
    offset: 0,
    limit: LOCAL_BATCH_PAGE_SIZE,
    revision: null,
  };
}
export interface LocalListCheckpoint {
  queryKey: string;
  scrollTop: number;
  groupScrollTop: number;
  expandedFolders: readonly string[];
}
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
  checked?: boolean;
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
  | { kind: 'rescan' }
  | { kind: 'hide_track'; id: string; revision: string }
  | { kind: 'hide_group'; query: LocalQuery; revision: string }
  | { kind: 'hide_tracks'; query: LocalQuery; revision: string; tracks: LocalTrackReference[] };
export type LocalBatchTransport = 'play' | 'add';
export interface LocalBatchPlayback {
  query: LocalQuery;
  revision: string;
  tracks: LocalTrackReference[];
}
export interface LocalTrackReference {
  id: string;
  revision: string;
}
export interface LocalDirectoryChoice {
  id: string;
  checked: boolean;
}
export interface LocalDirectorySelection {
  revision: string;
  entries: (LocalDirectoryChoice & { path: string; paused: boolean })[];
}
export interface LocalDirectoryClient {
  localDirectorySelection(): Promise<LocalDirectorySelection>;
  stageLocalDirectory(revision: string): Promise<LocalDirectorySelection | null>;
  applyLocalDirectories(
    revision: string,
    choices: LocalDirectoryChoice[],
  ): Promise<LocalDirectorySelection>;
}

export interface LocalClient {
  localPage(query: LocalQuery): Promise<LocalPage>;
  localCommand(command: LocalCommand): Promise<void>;
  pickLocalDirectory(): Promise<string | null>;
  openLocalTrackDirectory(id: string, revision: string): Promise<void>;
  revealLocalFolder(query: LocalQuery, revision: string): Promise<void>;
}

export type LocalCollectSort = 'default' | 'recently';
export interface LocalCollectPlan {
  receipt: string;
  count: number;
}
export interface LocalCollectResult {
  phase: PlaylistPhase;
  duplicate: boolean;
}
export interface LocalCollectClient {
  localCollectPrepare(
    epoch: string,
    selection: LocalBatchPlayback,
  ): Promise<AccountData<LocalCollectPlan>>;
  localCollectDestinations(
    epoch: string,
    receipt: string,
    sort: LocalCollectSort,
    page: number,
  ): Promise<AccountData<{ playlists: PlaylistView[]; nextPage: number | null }>>;
  localCollectExisting(
    epoch: string,
    receipt: string,
    target: string,
    reconcile: boolean,
  ): Promise<AccountData<LocalCollectResult>>;
  localCollectCancel(epoch: string, receipt: string): Promise<void>;
}
