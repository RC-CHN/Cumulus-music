import type { AccountData, AccountView, PlaylistView, TrackView } from './contracts';
import type { PlayerView, Quality } from './player';
import type { PlaylistMutationView } from './playlists';
export interface DownloadOptions {
  quality: Quality;
  tags: boolean;
  cover: boolean;
  lyrics: boolean;
  container: 'audio' | 'ncm' | 'official';
}
export const defaultDownloadOptions: DownloadOptions = {
  quality: 'standard',
  tags: true,
  cover: true,
  lyrics: false,
  container: 'audio',
};
export type DownloadTab = 'completed' | 'active';
export type SongDownloadState = 'none' | 'failed' | 'paused' | 'complete' | 'waiting' | 'working';
export type DownloadCategory = 'all' | 'song' | 'voice' | 'mv';
export type DownloadSort = 'name' | 'artist' | 'album' | 'size' | 'completed' | 'none';
export interface DownloadQuery {
  tab: DownloadTab;
  category?: DownloadCategory;
  keyword: string;
  sort: DownloadSort;
  descending: boolean;
  offset: number;
  limit: number;
}
export interface DirectorySettings {
  root: string;
  path_type: number;
  file_type: number;
  revision: string;
}
export interface DownloadTask {
  id: string;
  source: 'song' | 'cloud' | 'mv' | 'voice';
  track: TrackView;
  commentTrackId?: string;
  commentVoiceId?: string;
  linkTrackId?: string;
  collectionAvailable: boolean;
  voicePodcast?: { name: string; id?: string };
  status:
    | 'queued'
    | 'authorizing'
    | 'downloading'
    | 'processing'
    | 'paused'
    | 'error'
    | 'complete'
    | 'cancelling'
    | 'cancelled';
  phase: 'authorize' | 'transfer' | 'prepare' | 'publish' | 'complete';
  bytes: number;
  total: number | null;
  progress: number | null;
  speedBytesPerSecond?: number | null;
  requestedQuality: Quality;
  actualQuality: Quality | null;
  requestedResolution: number | null;
  actualResolution: number | null;
  encrypted: boolean;
  path: string | null;
  createdMs: number;
  completedMs: number | null;
  cleanupPending: boolean;
  needsTransferRestart?: boolean;
  fileOperation: string | null;
  lyricsStatus: 'pending' | 'prepared' | 'empty' | 'conflict' | 'complete' | 'error' | 'missing';
  error: string | null;
}
export interface FileOperation {
  id: string;
  task_id: string;
  action: 'move' | 'delete' | 'forget';
  phase: 'prepared' | 'cleanup' | 'complete' | 'cancelled';
  missing: string[];
  error: string | null;
  working: boolean;
  old_work_retained: boolean;
  files: {
    role: string;
    source: string;
    destination: string | null;
    stage: 'pending' | 'copying' | 'ready' | 'cleaned';
    bytes_copied: number;
    replacement_retained: boolean;
  }[];
}
export interface DirectoryMigration {
  id: string;
  source: string;
  target: string;
  phase: 'quiescing' | 'moving' | 'rolling_back' | 'complete' | 'cancelled';
  items: {
    task_id: string;
    operation_id: string;
    rollback_id: string;
    working: boolean;
    source: string | null;
    target: string | null;
    phase: 'pending' | 'complete' | 'rolled_back';
  }[];
  skipped: string[];
  waiting: string[];
  required_bytes: number;
  error: string | null;
}
export interface DownloadsView {
  epoch: string;
  tasks: DownloadTask[];
  total: number;
  activeCount: number;
  completedCount: number;
  offset: number;
  directory: DirectorySettings | null;
  options: DownloadOptions;
  migration: DirectoryMigration | null;
  operations: FileOperation[];
  jobs: {
    id: string;
    kind: 'file' | 'migration' | 'repair';
    running: boolean;
    error: string | null;
  }[];
}
export type DownloadControl = 'pause' | 'resume' | 'cancel' | 'repair';
export type DownloadCommand =
  | { kind: 'restart_failed_transfer'; task_id: string }
  | { kind: 'delete_files'; task_ids: string[] }
  | { kind: 'start_voice'; voice_ids: string[]; options: DownloadOptions }
  | { kind: 'start_mv'; id: string; resolution: number }
  | { kind: 'start_cloud'; song_ids: string[]; revision: string; lyrics: boolean }
  | { kind: 'start'; song_ids: string[]; options: DownloadOptions }
  | { kind: 'control'; task_ids: string[]; action: DownloadControl }
  | { kind: 'all'; action: 'pause' | 'resume' }
  | { kind: 'confirm_clear'; receipt: string }
  | { kind: 'preferences'; revision: string; options: DownloadOptions }
  | { kind: 'configure'; root: string; path_type: number; file_type: number }
  | { kind: 'migrate'; root: string }
  | { kind: 'migration' | 'file_operation'; operation_id: string; cancel: boolean }
  | { kind: 'file'; task_id: string; action: 'move' | 'delete' | 'forget'; target: string | null }
  | { kind: 'rename'; task_id: string; name: string }
  | { kind: 'move_to_directory'; task_id: string; root: string }
  | { kind: 'stop_operation'; operation_id: string };
export type DownloadSkipReason =
  | 'downloaded'
  | 'local'
  | 'waiting'
  | 'working'
  | 'cancelling'
  | 'options_changed'
  | 'existing'
  | 'missing'
  | 'conflict';
export interface DownloadResult {
  epoch: string;
  accepted: string[];
  failed: { id: string; message: string }[];
  skipped?: { id: string; reason: DownloadSkipReason }[];
  operationId: string | null;
}
export interface DownloadQualityPreferences {
  epoch: string;
  revision: string;
  options: DownloadOptions;
  choices: {
    quality: Quality;
    access: 'allowed' | 'requires_vip' | 'requires_svip' | 'unknown';
  }[];
}
export type DownloadShareAccess = 'allowed' | 'unavailable' | 'album_required';
export interface DownloadCollectScope {
  epoch: string;
  receipt: string;
  canCreate: boolean;
  ownedOnly?: boolean;
}
export interface DownloadBatchCollectPlan {
  receipt: string;
  acceptedTaskIds: string[];
}
export type DownloadCollectCreationCommand =
  | { kind: 'create'; name: string; privatePlaylist: boolean }
  | { kind: 'status'; reconcile: boolean }
  | { kind: 'add_created' };
export interface DownloadCollectCreation {
  receipt: string;
  create: PlaylistMutationView;
  add: PlaylistMutationView | null;
  canAdd: boolean;
}
export interface DownloadClearPlan {
  epoch: string;
  receipt: string | null;
  count: number;
}
export interface DownloadClient {
  downloads(epoch: string, query: DownloadQuery): Promise<DownloadsView>;
  downloadCommentCount(epoch: string, taskId: string): Promise<AccountData<number | null>>;
  downloadSongShareAccess(epoch: string, taskId: string): Promise<AccountData<DownloadShareAccess>>;
  downloadCollectPrepare(
    epoch: string,
    taskId: string,
  ): Promise<AccountData<Pick<DownloadCollectScope, 'receipt' | 'canCreate'>>>;
  downloadBatchCollectPrepare(
    epoch: string,
    taskIds: string[],
  ): Promise<AccountData<DownloadBatchCollectPlan>>;
  downloadCollectDestinations(
    epoch: string,
    receipt: string,
    sort: 'default' | 'recently',
    page: number,
  ): Promise<AccountData<{ playlists: PlaylistView[]; nextPage: number | null }>>;
  downloadCollectExisting(
    epoch: string,
    receipt: string,
    playlistId: string,
  ): Promise<AccountData<PlaylistMutationView>>;
  downloadCollectCreation(
    epoch: string,
    receipt: string,
    command: DownloadCollectCreationCommand,
  ): Promise<AccountData<DownloadCollectCreation>>;
  downloadCollectLikedDestination(
    epoch: string,
    receipt: string,
  ): Promise<AccountData<{ receipt: string } | null>>;
  downloadCollectLiked(
    epoch: string,
    receipt: string,
    destinationReceipt: string,
    reconcile: boolean,
  ): Promise<AccountData<PlaylistMutationView>>;
  downloadCollectCancel(epoch: string, receipt: string): Promise<void>;
  downloadQualityPreferences(epoch: string, refresh: boolean): Promise<DownloadQualityPreferences>;
  downloadClearPrepare(epoch: string): Promise<DownloadClearPlan>;
  downloadClearDiscard(epoch: string, receipt: string): Promise<void>;
  downloadCommand(epoch: string, command: DownloadCommand): Promise<DownloadResult>;
  playDownloads(epoch: string, taskIds: string[], index: number): Promise<PlayerView>;
  pickDownloadDirectory(epoch: string): Promise<string | null>;
  openDownloadDirectory(epoch: string, taskId: string | null): Promise<void>;
}
export type DownloadDialogTarget =
  | {
      kind: 'voice';
      tracks: Pick<import('./podcast').EpisodeView, 'id' | 'name'>[];
      live?: () => boolean;
    }
  | { kind: 'cloud'; tracks: TrackView[]; revision: string }
  | { kind: 'start'; tracks: TrackView[] }
  | {
      kind: 'voice_song';
      title: string;
      message: string;
      live: () => boolean;
      busy: () => boolean;
      start: (options: DownloadOptions) => Promise<boolean>;
    }
  | { kind: 'settings' }
  | { kind: 'file'; task: DownloadTask; action?: import('./downloadMenu').DownloadFileAction }
  | { kind: 'cancel'; task: DownloadTask }
  | { kind: 'restart'; task: DownloadTask }
  | { kind: 'clear' };
export function downloadStatus(task: DownloadTask): string {
  if (task.error) return task.error;
  if (task.fileOperation) return '正在整理文件';
  if (task.cleanupPending) return '歌曲已下载，附属文件待处理';
  return {
    queued: '等待下载',
    authorizing: '获取下载授权',
    downloading: '正在下载',
    processing: '正在整理音频',
    paused: '已暂停',
    error: '下载失败，可重试',
    complete: '已完成',
    cancelling: '正在取消',
    cancelled: '已取消',
  }[task.status];
}

export function downloadProgressStatus(task: DownloadTask, fresh: boolean): string {
  const rate = task.speedBytesPerSecond;
  if (
    !fresh ||
    task.status !== 'downloading' ||
    typeof rate !== 'number' ||
    !Number.isSafeInteger(rate) ||
    rate < 0
  )
    return downloadStatus(task);
  if (rate >= 1_000_000) return `${(rate / 1_000_000).toFixed(1)} MB/s`;
  if (rate >= 1_000) return `${Math.floor(Number((rate / 1_000).toFixed(2)))} KB/s`;
  return `${rate} B/s`;
}
export function bytesLabel(bytes: number | null): string {
  if (bytes === null) return '—';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
export function accountCanDownload(account: AccountView) {
  return account.status === 'authenticated';
}

export function downloadPlaybackReady(task: DownloadTask): boolean {
  return task.status === 'complete' && !!task.path && !task.fileOperation;
}
