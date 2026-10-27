import type { TrackView } from './contracts';
export type CloudTab = 'uploaded' | 'uploading';
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
  fileName: string;
  fileSize: number;
  addedAt: number;
  bitrate: number;
  format: string;
}
export interface CloudUpload {
  id: string;
  title: string;
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
  attachments: { kind: string; phase: string; bytes: number; total: number }[];
}
export interface CloudDelete {
  id: string;
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'partial' | 'rejected';
  total: number;
  removed: number;
  remaining: number;
}
export interface CloudPage {
  epoch: string;
  revision: string | null;
  updatedMs: number | null;
  size: number | null;
  maxSize: number | null;
  uploadedCount: number;
  uploadingCount: number;
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
  | { kind: 'pause' | 'cancel' | 'dismiss'; ids: string[] }
  | { kind: 'resume'; ids: string[]; retry_rejected: boolean }
  | { kind: 'renew' | 'reconcile_upload' | 'reconcile_delete'; id: string }
  | { kind: 'delete'; ids: string[]; revision: string };
export interface CloudResult {
  epoch: string;
  accepted: string[];
  failed: { id: string; message: string }[];
  deletion: CloudDelete | null;
}
export interface CloudClient {
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
        write_outcome_unknown: '服务端结果尚未确认，请先核对',
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
  return op.phase === 'confirmed'
    ? `已确认删除 ${op.removed} 首歌曲`
    : op.phase === 'partial'
      ? `已删除 ${op.removed} 首，${op.remaining} 首未删除`
      : op.phase === 'rejected'
        ? '删除未完成，歌曲仍保留在云盘'
        : `删除结果尚未确认，已核对 ${op.removed} 首；请核对剩余 ${op.remaining} 首`;
}
