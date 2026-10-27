import type { AccountData } from './contracts';
import type { CommentItem, CommentResource } from './comments';
export type CommentWriteCommand =
  | { kind: 'publish'; content: string; replyTo: string | null }
  | { kind: 'delete'; commentId: string };
export type CommentWritePhase =
  | 'prepared'
  | 'sending'
  | 'verifying'
  | 'uncertain'
  | 'confirmed'
  | 'rejected'
  | 'abandoned';
export interface CommentWrite {
  id: string;
  resource: CommentResource;
  command: CommentWriteCommand;
  phase: CommentWritePhase;
  createdAt: number;
  acknowledged: boolean;
  receiptId: string | null;
  target: CommentItem | null;
  observed: CommentItem | null;
  observedAbsent: boolean;
  serviceCode: number | null;
}
export type CommentWriteAction = { kind: 'submit' | 'reconcile' | 'abandon' };
export interface CommentWriteClient {
  commentsPrepareWrite(
    epoch: string,
    resource: CommentResource,
    command: CommentWriteCommand,
  ): Promise<AccountData<CommentWrite>>;
  commentsWriteOperations(
    epoch: string,
    resource: CommentResource,
  ): Promise<AccountData<CommentWrite[]>>;
  commentsWriteCommand(
    epoch: string,
    id: string,
    command: CommentWriteAction,
  ): Promise<AccountData<CommentWrite>>;
}
export function commentWritePending(row: CommentWrite) {
  return ['prepared', 'sending', 'verifying', 'uncertain'].includes(row.phase);
}
export function commentWriteMessage(row: CommentWrite) {
  if (row.phase === 'confirmed')
    return row.command.kind === 'delete' ? '评论已删除并确认。' : '评论已发布并确认。';
  if (row.phase === 'prepared') return '操作已保存，尚未发送。';
  if (row.phase === 'abandoned') return '已结束处理；这不会撤回可能已发送的内容。';
  if (row.phase === 'rejected')
    return row.serviceCode === 480
      ? '发布前需要绑定手机号。'
      : row.serviceCode === 404
        ? '原评论已不可用。'
        : '服务未接受这次操作，请检查内容后再试。';
  if (row.command.kind === 'delete' && row.observedAbsent)
    return '评论当前不可见，但删除结果尚未确认。';
  return row.acknowledged
    ? '服务已接收，显示结果尚待确认，请稍后核对。'
    : '提交结果未确认，请先查看评论并核对，不要重复提交。';
}
