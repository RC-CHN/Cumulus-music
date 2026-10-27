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
  | { kind: 'reconcile'; operation_id: string };
export interface TogetherClient {
  onTogetherCloseError?(epoch: string, handler: (message: string) => void): Promise<() => void>;
  togetherView(epoch: string, refresh: boolean): Promise<AccountData<TogetherView>>;
  togetherCommand(epoch: string, command: TogetherCommand): Promise<AccountData<TogetherView>>;
}
