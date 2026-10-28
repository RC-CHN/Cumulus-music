import type { AccountData } from './contracts';
import type { Quality } from './player';
import type { SettingsOperation } from './settings';
export interface RelaySettingOperation
  extends Omit<SettingsOperation, 'change' | 'before' | 'observed'> {
  enabled: boolean;
  before: boolean | null;
  observed: boolean | null;
}
export interface RelaySettingState {
  revision: string;
  enabled: boolean | null;
  canChange: boolean;
  operations: RelaySettingOperation[];
}
export type RelaySettingCommand =
  | {
      kind: 'setting_change' | 'setting_cancel_unseen';
      id: string;
      revision: string;
      enabled: boolean;
    }
  | { kind: 'setting_reconcile'; id: string }
  | { kind: 'setting_accept'; id: string; revision: string };
export function isRelaySettingCommand(
  command: RelayCommand | RelaySettingCommand,
): command is RelaySettingCommand {
  return command.kind.startsWith('setting_');
}
export const relaySettingPending = (operation: RelaySettingOperation) =>
  operation.phase === 'pending' || operation.phase === 'uncertain';
export interface RelayOperation {
  id: string;
  phase: 'preparing' | 'applying' | 'installed' | 'failed' | 'uncertain' | 'cancelled';
  pullPhase: 'pending' | 'ready' | 'rejected' | 'uncertain' | 'cancelled' | null;
  canRetry: boolean;
  superseded?: boolean;
  createdAt: number;
}
export interface RelayView {
  revision: string;
  availability: 'unchecked' | 'experiment_excluded' | 'server_disabled' | 'available';
  setting: boolean | null;
  settings?: RelaySettingState | null;
  alert: { title: string; deviceName: string; resourceName: string; buttonText: string } | null;
  operations: RelayOperation[];
}
export type RelayCommand =
  | { kind: 'continue'; id: string; revision: string; quality: Quality }
  | { kind: 'retry'; id: string; previous_id: string; quality: Quality }
  | { kind: 'dismiss'; revision: string }
  | { kind: 'cancel_unseen'; id: string; revision: string };
export interface RelayClient {
  relayView(epoch: string, refresh: boolean): Promise<AccountData<RelayView>>;
  relayCommand(epoch: string, command: RelayCommand): Promise<AccountData<RelayView>>;
  relaySettingCommand(epoch: string, command: RelaySettingCommand): Promise<AccountData<RelayView>>;
}
export function relayOutcome(operation: RelayOperation) {
  if (operation.superseded) return '这次准备已由后续重试处理';
  if (operation.phase === 'installed') return '接续内容已载入当前队列';
  if (operation.phase === 'cancelled') return '已取消尚未开始的接续';
  if (operation.phase === 'preparing') return '正在准备接续内容';
  if (operation.phase === 'applying') return '正在更新播放队列';
  if (operation.canRetry) return '未完成本机播放，可重新准备已取回的内容';
  if (operation.pullPhase === 'rejected') return '设备接续请求被服务端拒绝';
  return '未确认接续结果，未自动重试。可重新检查其他设备';
}
