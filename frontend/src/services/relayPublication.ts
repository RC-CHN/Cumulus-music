import type { AccountData } from './contracts';
export interface RelayPublicationReceipt {
  id: string;
  phase:
    | 'prepared'
    | 'sending'
    | 'confirmed'
    | 'rejected'
    | 'uncertain'
    | 'skipped'
    | 'superseded'
    | 'cancelled';
  recoveryOf: string | null;
  serviceCode: number | null;
  createdAt: number;
}
export interface RelayPublicationView {
  pending: number;
  blocked: RelayPublicationReceipt | null;
  recovering: RelayPublicationReceipt | null;
  latest: RelayPublicationReceipt | null;
  requested: RelayPublicationReceipt | null;
  canResynchronize: boolean;
  sending: boolean;
  recordFailed: boolean;
  preflightFailed: boolean;
}
export type RelayPublicationCommand = {
  kind: 'resynchronize' | 'cancel_unseen';
  id: string;
  blocked_id: string;
};
export interface RelayPublicationClient {
  relayPublicationView(
    epoch: string,
    requested: string | null,
  ): Promise<AccountData<RelayPublicationView>>;
  relayPublicationCommand(
    epoch: string,
    command: RelayPublicationCommand,
  ): Promise<AccountData<RelayPublicationView>>;
}
export function publicationOutcome(view: RelayPublicationView) {
  if (view.recordFailed) return '当前播放记录暂时无法保存，已暂停同步；本机播放不受影响。';
  if (view.recovering)
    return view.preflightFailed ? '等待连接恢复，当前队列尚未重新同步。' : '正在重新同步当前队列…';
  if (view.blocked)
    return view.blocked.phase === 'rejected'
      ? '服务端拒绝了同步请求，已暂停后续同步。'
      : '未确认同步结果，已暂停后续同步。';
  if (view.preflightFailed) return '暂时无法连接续播服务，本机播放不受影响。';
  if (view.sending || view.pending) return '正在同步当前播放…';
  if (view.latest?.phase === 'confirmed') return '最近一次播放记录已同步。';
  if (view.latest?.phase === 'skipped') return '服务端暂未允许同步，本机播放不受影响。';
  return '播放符合条件的在线音乐后，会同步本机的播放记录。';
}
