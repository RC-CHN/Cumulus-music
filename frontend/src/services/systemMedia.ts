import type { AccountData } from './contracts';
export interface SystemMediaPreferencesView {
  revision: string | null;
  enabled: boolean;
  damaged: boolean;
}
export type SystemMediaPreferencesCommand =
  | { kind: 'change'; expected_revision: string; enabled: boolean }
  | { kind: 'repair' };
export interface SystemMediaPreferencesClient {
  systemMediaPreferences(epoch: string): Promise<AccountData<SystemMediaPreferencesView>>;
  systemMediaPreferencesCommand(
    epoch: string,
    command: SystemMediaPreferencesCommand,
  ): Promise<AccountData<SystemMediaPreferencesView>>;
}
export type SystemMediaPhase =
  | 'not_installed'
  | 'starting'
  | 'disabled'
  | 'damaged'
  | 'read_failed'
  | 'idle'
  | 'connecting'
  | 'active'
  | 'unavailable';
export interface SystemMediaStatus {
  revision: string;
  installed: boolean;
  phase: SystemMediaPhase;
}
export interface SystemMediaStatusClient {
  systemMediaStatus(): Promise<SystemMediaStatus>;
  onSystemMediaStatus(handler: (view: SystemMediaStatus) => void): Promise<() => void>;
}
export interface SystemMediaClient extends SystemMediaPreferencesClient, SystemMediaStatusClient {}
export const systemMediaLabels: Record<SystemMediaPhase, string> = {
  not_installed: '此环境暂不提供系统媒体控制。',
  starting: '正在读取本机设置…',
  disabled: '系统媒体控制已关闭。',
  damaged: '本机设置已损坏，系统媒体控制已停用。',
  read_failed: '暂时无法读取本机设置，系统媒体控制已停用。',
  idle: '开始播放后，可从系统媒体面板控制音乐。',
  connecting: '正在连接系统媒体服务…',
  active: '系统媒体控制已连接。',
  unavailable: '系统媒体服务暂不可用，仍可在 Cumulus 中控制播放。',
};
