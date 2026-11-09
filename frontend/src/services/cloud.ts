import type { TrackView } from './contracts';
export type CloudTab = 'uploaded' | 'uploading' | 'history';
export interface CloudQuery {
  tab: CloudTab;
  keyword: string;
  sort: 'natural' | 'uploaded' | 'title' | 'artist' | 'album' | 'format' | 'size';
  descending: boolean;
  offset: number;
  limit: number;
}
// M/106 quinary/trinary + M/195 artistForward/artistReverse projection.
export function nextCloudSort(
  query: Pick<CloudQuery, 'sort' | 'descending'>,
  column: Exclude<CloudQuery['sort'], 'natural'>,
): Pick<CloudQuery, 'sort' | 'descending'> {
  if (column === 'title' && (query.sort === 'title' || query.sort === 'artist')) {
    if (!query.descending) return { sort: query.sort, descending: true };
    return query.sort === 'title'
      ? { sort: 'artist', descending: false }
      : { sort: 'natural', descending: false };
  }
  if (query.sort !== column) return { sort: column, descending: false };
  return query.descending
    ? { sort: 'natural', descending: false }
    : { sort: column, descending: true };
}
export type CloudPhase =
  | 'queued'
  | 'checked'
  | 'allocation_pending'
  | 'allocated'
  | 'audio_uploading'
  | 'audio_uploaded'
  | 'attachments_uploading'
  | 'metadata_pending'
  | 'encoding'
  | 'ready_to_publish'
  | 'publication_pending'
  | 'published'
  | 'complete'
  | 'failed'
  | 'cancelled';
export interface CloudTrack {
  track: TrackView;
  deletionKind?: 'ordinary' | 'private' | 'off_store' | 'unknown';
  publicSongId?: string | null;
  fileName: string;
  fileSize: number;
  addedAt: number;
  bitrate: number;
  format: string;
}
export interface CloudResource {
  epoch: string;
  revision: string;
  track: CloudTrack;
  localAvailable: boolean;
  localIssue?: string | null;
}
export interface CloudAttachmentOmission {
  attachment: 'lyric' | 'cue';
  revision: string;
}
export function cloudTextAttachmentLabel(attachment: CloudAttachmentOmission['attachment']) {
  return attachment === 'lyric' ? '歌词' : 'CUE 曲目表';
}
export interface CloudUpload {
  id: string;
  title: string;
  artists?: string[];
  album?: string;
  fileName: string;
  phase: CloudPhase;
  bytes: number;
  total: number;
  label: string;
  enabled: boolean;
  running: boolean;
  error: string | null;
  canResume: boolean;
  canPause: boolean;
  canCancel: boolean;
  canReconcile: boolean;
  canRenew: boolean;
  canDismiss: boolean;
  removalPending?: boolean;
  canRemove?: boolean;
  canRetryRemove?: boolean;
  canRetryEncoding?: boolean;
  omittableAttachment?: CloudAttachmentOmission | null;
  attachments: { kind: string; phase: string; bytes: number; total: number }[];
}
export interface CloudDelete {
  id: string;
  ids: string[];
  phase:
    | 'preparing'
    | 'not_submitted'
    | 'sending'
    | 'verifying'
    | 'uncertain'
    | 'confirmed'
    | 'partial'
    | 'rejected';
  total: number;
  removed: number;
  remaining: number;
}
export interface CloudRemoval {
  epoch: string;
  id: string;
  revision: string;
  ids: string[];
  titles: string[];
  kinds: NonNullable<CloudTrack['deletionKind']>[];
  phase: 'prepared' | 'local' | 'remote' | 'result';
  running: boolean;
  deleteLocal: boolean | null;
  localCount: number;
  cueCount: number;
  localIssue: string | null;
  files: {
    cloudId: string;
    title: string;
    state: 'pending' | 'removing' | 'removed' | 'missing' | 'retained' | 'failed' | 'skipped';
    issue: string | null;
  }[];
  deletion: CloudDelete | null;
  legacyOnly: boolean;
  canContinue: boolean;
  error: string | null;
}
export interface CloudCapacityStatus {
  kind: 'full' | 'permanent' | 'svip_active' | 'svip_expiring' | 'svip_offer';
  text: string;
}
export interface CloudPage {
  epoch: string;
  revision: string | null;
  updatedMs: number | null;
  size: number | null;
  maxSize: number | null;
  capacityStatus?: CloudCapacityStatus | null;
  uploadedCount: number;
  uploadingCount: number;
  historyCount?: number;
  total: number;
  offset: number;
  tracks: CloudTrack[];
  uploads: CloudUpload[];
  deletions: CloudDelete[];
  workerError: string | null;
}
export interface CloudUploadInput {
  path: string;
  cover: string | null;
  lyric: string | null;
  cue: string | null;
}
export type CloudCommand =
  | { kind: 'pause_all' }
  | { kind: 'resume_all'; retry_rejected: boolean }
  | { kind: 'upload'; files: CloudUploadInput[] }
  | { kind: 'upload_planned'; files: { request_id: string; input: CloudUploadInput }[] }
  | { kind: 'pause' | 'cancel' | 'dismiss'; ids: string[] }
  | { kind: 'resume'; ids: string[]; retry_rejected: boolean }
  | { kind: 'remove_uploads'; ids: string[]; retry: boolean }
  | { kind: 'renew' | 'reconcile_upload' | 'reconcile_delete' | 'retry_encoding'; id: string }
  | {
      kind: 'omit_text_attachment';
      id: string;
      attachment: CloudAttachmentOmission['attachment'];
      expected_revision: string;
    }
  | { kind: 'delete'; ids: string[]; revision: string };
export interface CloudResult {
  epoch: string;
  accepted: string[];
  failed: { id: string; message: string }[];
  deletion: CloudDelete | null;
  notStarted?: { id: string; message: string }[];
}
export interface CloudUploadRemoval {
  epoch: string;
  items: { id: string; removed: boolean; requested: boolean; error: string | null }[];
}
export interface CloudClient {
  cloudDeletePrepare(epoch: string, ids: string[], revision: string): Promise<CloudRemoval>;
  cloudDeleteView(epoch: string, id: string, reconcile: boolean): Promise<CloudRemoval>;
  cloudDeleteExecute(
    epoch: string,
    id: string,
    deleteLocal: boolean,
    retryLocal: boolean,
  ): Promise<CloudRemoval>;
  cloudDeleteDiscard(epoch: string, id: string): Promise<void>;
  openCloudCapacity(epoch: string): Promise<void>;
  cloudUploadSelection(epoch: string, history: boolean): Promise<CloudPage>;
  cloudUploadRemovals(epoch: string, ids: string[]): Promise<CloudUploadRemoval>;
  openCloudUploadDirectory(epoch: string, id: string): Promise<void>;

  cloudResource(epoch: string, songId: string, revision: string): Promise<CloudResource>;
  openCloudDirectory(epoch: string, songId: string, revision: string): Promise<void>;
  cloudPage(epoch: string, query: CloudQuery, refresh: boolean): Promise<CloudPage>;
  cloudCommand(epoch: string, command: CloudCommand): Promise<CloudResult>;
  pickCloudFiles(
    epoch: string,
    kind: 'audio' | 'folder' | 'cover' | 'lyric' | 'cue',
  ): Promise<string[]>;
}
export function cloudIssue(code: string | null): string {
  if (!code) return '';
  return (
    (
      {
        encoding_failed: '云端转码暂未成功，可重新检查转码状态',
        write_outcome_unknown: '服务端结果尚未确认，请先核对',
        nos_boundary_unverified:
          '服务端未确认首片或末片，无法核对或继续上传。可以停止本地任务；服务端对象不会被删除。',
        source_changed: '本地文件已变化，请重新选择文件',
        file_unavailable: '本地文件无法读取，请检查位置与权限',
        network: '网络请求失败，请稍后继续',
        service_rejected: '服务未允许此次操作',
        object_credentials_rejected: '上传凭证已过期或被拒绝',
        image_only_bucket_for_text: '服务分配的存储位置不支持文本附件',
        source_or_checksum_mismatch: '文件或校验结果不一致',
        unsupported_upload_channel: '服务返回了尚不支持的上传通道',
      } as Record<string, string>
    )[code] ?? '上传未完成，请检查文件和网络后继续'
  );
}
export function cloudBytes(value: number | null): string {
  if (value === null) return '—';
  if (value < 1024) return `${value} B`;
  const unit = Math.min(3, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** unit).toFixed(unit > 1 ? 2 : 0)} ${['B', 'KB', 'MB', 'GB'][unit]}`;
}
export function deletionMessage(op: CloudDelete): string {
  if (op.phase === 'not_submitted') return '云盘列表已变化，本次删除未提交';
  if (op.phase === 'preparing') return '云盘删除尚未提交，可继续已确认的操作';
  return op.phase === 'confirmed'
    ? `已确认删除 ${op.removed} 首歌曲`
    : op.phase === 'partial'
      ? `已删除 ${op.removed} 首，${op.remaining} 首未删除`
      : op.phase === 'rejected'
        ? '删除未完成，歌曲仍保留在云盘'
        : `删除结果尚未确认，已核对 ${op.removed} 首；请核对剩余 ${op.remaining} 首`;
}
