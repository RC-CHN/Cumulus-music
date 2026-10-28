import type { AccountData } from './contracts';
export type SettingKey =
  | 'shareSetting'
  | 'playRecordSetting'
  | 'personalServiceSetting'
  | 'allowSubscriptionNotify'
  | 'allowLikedNotify'
  | 'allowNewFollowerNotify'
  | 'allowPeopleCanSeeMyPlayRecord';
export interface SettingChange {
  key: SettingKey;
  value: boolean | number;
}
export interface SettingField {
  key: SettingKey;
  value: boolean | number | null;
  state: 'available' | 'missing' | 'unsupported';
}
export interface SettingsOperation {
  id: string;
  change: SettingChange;
  before: boolean | number | null;
  observed: boolean | number | null;
  phase:
    | 'pending'
    | 'confirmed'
    | 'rejected'
    | 'uncertain'
    | 'unchanged'
    | 'cancelled'
    | 'accepted_current_state';
  createdAt: number;
  lastReadAt: number | null;
  readRevision: string | null;
}
export interface AccountSettingsPage {
  revision: string | null;
  fetchedAt: number | null;
  fields: SettingField[];
  linkedPrivacy: {
    peopleCanSeePlayRecord: boolean | null;
    followedCanSeePlayRecord: boolean | null;
    socialPlaylistSetting: number | null;
  };
  operations: SettingsOperation[];
}
export type AccountSettingsCommand =
  | {
      kind: 'change' | 'cancel_unseen';
      id: string;
      expected_revision: string;
      change: SettingChange;
    }
  | { kind: 'reconcile'; id: string }
  | { kind: 'accept_current'; id: string; expected_revision: string };
export interface SettingsClient {
  accountSettingsPage(epoch: string, refresh: boolean): Promise<AccountData<AccountSettingsPage>>;
  accountSettingsCommand(
    epoch: string,
    command: AccountSettingsCommand,
  ): Promise<AccountData<AccountSettingsPage>>;
}
export const settingLabels: Record<SettingKey, string> = {
  shareSetting: '谁可以给我发私信',
  playRecordSetting: '我的听歌排行',
  personalServiceSetting: '个性化服务',
  allowSubscriptionNotify: '歌单被收藏',
  allowLikedNotify: '收到赞',
  allowNewFollowerNotify: '新粉丝',
  allowPeopleCanSeeMyPlayRecord: '允许所有人查看听歌记录',
};
export const settingsPending = (op: SettingsOperation) =>
  op.phase === 'pending' || op.phase === 'uncertain';
export function settingValueLabel(key: SettingKey, value: boolean | number | null) {
  if (value === null) return '服务端未返回可识别的值';
  if (typeof value === 'boolean') return value ? '开启' : '关闭';
  if (key === 'personalServiceSetting') return value === 0 ? '开启' : '关闭';
  if (key === 'shareSetting') return value === 0 ? '所有人' : '我关注的人';
  return ['所有人可见', '我关注的人可见', '仅自己可见'][value] ?? '未知';
}
export function settingsOutcome(op: SettingsOperation) {
  switch (op.phase) {
    case 'pending':
    case 'uncertain':
      return '修改结果待核对';
    case 'confirmed':
      return '已回读并确认保存';
    case 'unchanged':
      return '当前已是所选状态';
    case 'rejected':
      return '服务端未接受这次修改';
    case 'cancelled':
      return '未开始的修改已取消';
    case 'accepted_current_state':
      return '已保留当前设置';
  }
}
