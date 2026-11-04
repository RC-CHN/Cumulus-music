import type { AccountData } from './contracts';

export interface ProfileBlacklistEntry {
  id: string;
  nickname: string;
  avatarUrl: string | null;
  signature: string;
  closed: boolean;
}

export interface ProfileBlacklistHeader {
  blocked: boolean;
  canAdd: boolean;
  pending: boolean;
  busyWithAnother: boolean;
}

export interface ProfileBlacklistIntent {
  id: string;
  desired: boolean;
}

export interface ProfileBlacklistList {
  users: ProfileBlacklistEntry[];
  pending: ProfileBlacklistIntent | null;
}

export type ProfileBlacklistStatus =
  | 'added'
  | 'removed'
  | 'unchanged'
  | 'already_blocked'
  | 'full'
  | 'unavailable'
  | 'rejected'
  | 'uncertain';

export type ProfileBlacklistCommand =
  | { kind: 'change'; expected: boolean; origin: 'profile' | 'list' }
  | { kind: 'reconcile' };

export interface ProfileBlacklistResult {
  status: ProfileBlacklistStatus;
  blocked: boolean | null;
  pending: ProfileBlacklistIntent | null;
  users: ProfileBlacklistEntry[] | null;
}

export interface ProfileBlacklistClient {
  profileBlacklistList(epoch: string): Promise<AccountData<ProfileBlacklistList>>;
  profileBlacklist(
    epoch: string,
    uid: string,
    command: ProfileBlacklistCommand,
  ): Promise<AccountData<ProfileBlacklistResult>>;
}

export const blacklistMessage: Record<ProfileBlacklistStatus, string> = {
  added: '已加入黑名单',
  removed: '已解除黑名单',
  unchanged: '名单状态已改变，请查看当前状态',
  already_blocked: '该用户已经加入黑名单了',
  full: '黑名单已满，请在设置中整理后再试',
  unavailable: '暂时无法将该用户加入黑名单',
  rejected: '操作未发送，请核对账号状态后重试',
  uncertain: '结果尚未确认，请先核对黑名单',
};
