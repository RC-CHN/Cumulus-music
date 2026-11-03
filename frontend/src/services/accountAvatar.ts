import type { AccountData } from './contracts.ts';
import type { CoverCrop, CoverImage } from './playlistCover.ts';

export type AvatarPhase =
  | 'draft'
  | 'prepared'
  | 'allocating'
  | 'uploading'
  | 'uploaded'
  | 'binding'
  | 'confirmed'
  | 'rejected'
  | 'abandoned'
  | 'accepted_current_state';

export interface AvatarTask {
  id: string;
  phase: AvatarPhase;
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
  error: string | null;
  mayLeaveObject: boolean;
  cleanupPending: boolean;
  metadataPending: boolean;
  observedRevision: string | null;
  observedAvatarUrl: string | null;
}
export type AvatarCommand =
  | { kind: 'save'; crop: CoverCrop }
  | { kind: 'accept_current'; expectedRevision: string }
  | { kind: 'continue' | 'reconcile' | 'abandon' | 'cleanup' };

export interface AccountAvatarClient {
  accountAvatarTasks(epoch: string): Promise<AccountData<AvatarTask[]>>;
  accountAvatarImage(epoch: string, id: string): Promise<AccountData<CoverImage>>;
  pickAccountAvatar(epoch: string): Promise<AccountData<AvatarTask | null>>;
  accountAvatarCommand(
    epoch: string,
    id: string,
    command: AvatarCommand,
  ): Promise<AccountData<AvatarTask>>;
}

export const avatarDraft = (task?: AvatarTask) =>
  task?.phase === 'draft' || task?.phase === 'prepared';
export const avatarTerminal = (task: AvatarTask) =>
  ['confirmed', 'rejected', 'abandoned', 'accepted_current_state'].includes(task.phase);
export const avatarContinue = (task: AvatarTask) =>
  (task.phase === 'uploaded' && task.error !== 'avatar_changed') ||
  (task.phase === 'uploading' && task.transfer !== 'ambiguous' && task.transfer !== 'cancelled');
export const avatarCanAbandon = (task: AvatarTask) =>
  !avatarTerminal(task) && task.phase !== 'binding';
export const avatarTask = (tasks: AvatarTask[]) =>
  tasks.find((task) => !avatarTerminal(task)) ??
  tasks.find(({ cleanupPending, metadataPending }) => cleanupPending || metadataPending);

export function avatarMessage(task: AvatarTask) {
  if (task.error === 'avatar_rate_limited') return '已达到头像修改次数限制，请稍后再试。';
  if (task.error === 'avatar_phone_required') return '服务要求先绑定手机号，才能修改个人资料。';
  if (task.error === 'avatar_bind_not_sent') return '未能提交头像，本次没有替换。';
  if (task.error === 'avatar_changed') return '当前头像已在其他位置更新，本次没有替换。';
  if (task.phase === 'uploading' && task.transfer === 'ambiguous')
    return '图片上传结果尚未确认，头像尚未提交。';
  if (task.phase === 'uploading' && task.transfer === 'error') return '图片上传中断，可继续处理。';
  if (task.phase === 'uploading' && task.transfer === 'paused')
    return '图片上传已暂停，可继续处理。';
  return (
    {
      draft: '',
      prepared: '',
      allocating: '上传准备结果尚未确认，头像尚未提交。',
      uploading: '图片尚未上传完成。',
      uploaded: '图片已上传，尚未提交头像。',
      binding: '头像修改结果尚未确认。请核对当前头像，避免重复提交。',
      confirmed: '头像已更新。',
      rejected: '此次头像修改未完成。',
      abandoned: '已放弃本次头像编辑。',
      accepted_current_state: '已采用当前头像。',
    }[task.phase] ?? '暂时无法确认头像编辑进度。'
  );
}
