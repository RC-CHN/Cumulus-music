import type { AccountData } from './contracts';
import type { PlaylistEditor } from './playlists';
export interface CoverCrop {
  x: number;
  y: number;
  size: number;
}
export interface CoverImage {
  width: number;
  height: number;
  crop: CoverCrop;
  preview: string;
}
export type CoverPhase =
  | 'draft'
  | 'prepared'
  | 'allocating'
  | 'allocation_rejected'
  | 'uploading'
  | 'uploaded'
  | 'binding'
  | 'confirmed'
  | 'rejected'
  | 'abandoned';
export interface CoverTask {
  id: string;
  playlistId: string;
  phase: CoverPhase;
  crop: CoverCrop | null;
  width: number;
  height: number;
  bytes: number;
  total: number;
  transfer:
    | 'queued'
    | 'uploading'
    | 'paused'
    | 'error'
    | 'ambiguous'
    | 'uploaded'
    | 'cancelled'
    | null;
  binding: string | null;
  error: string | null;
  mayLeaveObject: boolean;
  cleanupPending: boolean;
}
export type CoverCommand =
  | { kind: 'save'; crop: CoverCrop }
  | { kind: 'rebase'; revision: string }
  | {
      kind:
        | 'continue'
        | 'retry_allocation'
        | 'reconcile'
        | 'pause'
        | 'renew'
        | 'abandon'
        | 'cleanup';
    };
export interface CoverOutcome {
  task: CoverTask;
  editor: PlaylistEditor | null;
  warning: string | null;
}
export interface PlaylistCoverClient {
  playlistCoverTasks(epoch: string): Promise<AccountData<CoverTask[]>>;
  playlistCoverImage(epoch: string, id: string): Promise<AccountData<CoverImage>>;
  pickPlaylistCover(
    epoch: string,
    playlistId: string,
    revision: string,
    replaceId: string | null,
  ): Promise<AccountData<CoverTask | null>>;
  playlistCoverCommand(
    epoch: string,
    id: string,
    command: CoverCommand,
  ): Promise<AccountData<CoverOutcome>>;
}
export const coverTerminal = (task: CoverTask) =>
  ['confirmed', 'rejected', 'abandoned'].includes(task.phase);
export const coverEditable = (task: CoverTask) => ['draft', 'prepared'].includes(task.phase);
export function coverMessage(task: CoverTask) {
  if (task.phase === 'uploading' && task.transfer === 'ambiguous')
    return '图片上传结果未确认，尚未修改歌单封面。可以放弃本次换图。';
  if (task.phase === 'uploading' && task.transfer === 'error')
    return '图片上传已中断，可以重试；若上传权限失效，可重新获取权限。';
  if (task.phase === 'uploading' && task.transfer === 'paused')
    return '图片上传已暂停，可继续完成保存。';
  if (task.error === 'playlist_changed')
    return '歌单已在其他位置更新，请先查看最新信息，再确认应用这张封面。';
  return {
    draft: '拖动图片调整位置，缩放选择需要保留的部分。',
    prepared: '图片已准备好，可以继续保存。',
    allocating: '图片上传准备结果未确认，尚未修改歌单封面。可以放弃本次换图。',
    allocation_rejected: '图片上传申请被拒绝，可以稍后重试。',
    uploading: '正在上传图片…',
    uploaded: '图片已上传，等待保存到歌单。',
    binding: '封面更新结果待核对，请勿重复换图。',
    confirmed: '封面已保存并确认。',
    rejected: '此次封面更新未完成。',
    abandoned: '已放弃本次换图。',
  }[task.phase];
}

export function coverBusyMessage(task: CoverTask | undefined, stopping: boolean) {
  if (stopping) return '正在暂停上传，等待当前数据传输结束…';
  if (task?.phase === 'allocating') return '正在准备上传图片…';
  if (task?.phase === 'uploading') return '正在上传图片…';
  if (task?.phase === 'uploaded' || task?.phase === 'binding') return '正在保存封面并核对结果…';
  return '正在处理并核对封面，请稍候…';
}
