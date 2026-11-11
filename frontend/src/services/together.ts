import type { AccountData, TrackView } from './contracts';
export interface TogetherMember {
  nickname: string;
  avatarUrl: string | null;
  isSelf: boolean;
}
export interface TogetherRoom {
  id: string;
  role: 'waiting' | 'together_owner' | 'together';
  members: TogetherMember[];
  relationKey: string | null;
}
export interface TogetherStatistics {
  listenCount: number;
  totalSeconds: number;
}
export interface TogetherLeaveRecord {
  operationId: string;
  thisListenCount: number;
  thisDuration: number;
  totalListenCount: number;
  totalDuration: number;
  selfAvatarUrl: string | null;
  peerAvatarUrl: string | null;
}
export function togetherRecordDuration(seconds: number) {
  // Official M103.i takes seconds, shows a minimum minute and no seconds.
  const totalMinutes = Math.floor(seconds / 60);
  const hours = Math.floor(totalMinutes / 60);
  return hours ? `${hours}小时${totalMinutes % 60}分钟` : `${totalMinutes || 1}分钟`;
}
export function confirmedTogetherLeaveRecord(view: TogetherView): TogetherLeaveRecord | undefined {
  const record = view.leaveRecord;
  if (
    view.room ||
    view.ending ||
    view.pending.some((operation) => operation.action === 'end') ||
    !record ||
    typeof record.operationId !== 'string' ||
    !record.operationId ||
    record.operationId.length > 128 ||
    ![
      record.thisListenCount,
      record.thisDuration,
      record.totalListenCount,
      record.totalDuration,
    ].every((value) => Number.isSafeInteger(value) && value >= 0)
  )
    return;
  return {
    operationId: record.operationId,
    thisListenCount: record.thisListenCount,
    thisDuration: record.thisDuration,
    totalListenCount: record.totalListenCount,
    totalDuration: record.totalDuration,
    selfAvatarUrl: record.selfAvatarUrl,
    peerAvatarUrl: record.peerAvatarUrl,
  };
}
export interface TogetherOperation {
  id: string;
  action: 'create' | 'accept' | 'end' | 'queue' | 'play';
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'rejected' | 'room_closed';
  acknowledged: boolean;
}
export interface TogetherView {
  room: TogetherRoom | null;
  tracks: TrackView[];
  mode: 'ORDER_LOOP' | 'RANDOM' | 'SINGLE_LOOP';
  targetId: string | null;
  progressMs: number | null;
  playing: boolean;
  attached: boolean;
  realtimeReady: boolean;
  connection: {
    state:
      | 'unavailable'
      | 'idle'
      | 'connecting'
      | 'entering'
      | 'connected'
      | 'reconnecting'
      | 'failed'
      | 'closing'
      | 'closed';
    message: string | null;
    received: number;
    attempt: number;
  };
  pending: TogetherOperation[];
  syncError: string | null;
  checkedAtMs: number | null;
  localRecovery: boolean;
  ending: boolean;
  externalSelection?: { id: string; roomId: string; skipped: number; blocked: boolean };
  queueRoomLoss?: { roomId: string; reportId: string } | null;
  leaveRecord?: TogetherLeaveRecord;
  lifecycleNotice: string | null;
  sharedPhase: 'pending' | 'applied' | 'superseded' | 'failed' | null;
  nativeTargetId: string | null;
  commonAccess: {
    trackId: string;
    canPlay: boolean;
    reason: string | null;
    preview: { start: number; end: number } | null;
    limitedMember: 'none' | 'me' | 'other' | 'both' | 'unknown';
  }[];
}
export type TogetherCommand =
  | { kind: 'create' | 'restore_local' }
  | { kind: 'attach' | 'publish' | 'end' | 'reconnect'; room_id: string }
  | { kind: 'select' | 'remove'; room_id: string; song_id: string }
  | { kind: 'confirm_queue_room_loss'; room_id: string; report_id: string }
  | { kind: 'resolve_external_selection'; room_id: string; notice_id: string; confirm: boolean }
  | { kind: 'reconcile' | 'dismiss_leave_record'; operation_id: string };
export interface TogetherClient {
  onTogetherCloseError?(epoch: string, handler: (message: string) => void): Promise<() => void>;
  togetherView(epoch: string, refresh: boolean): Promise<AccountData<TogetherView>>;
  togetherStatistics(epoch: string, relationKey: string): Promise<AccountData<TogetherStatistics>>;
  togetherCommand(epoch: string, command: TogetherCommand): Promise<AccountData<TogetherView>>;
}
