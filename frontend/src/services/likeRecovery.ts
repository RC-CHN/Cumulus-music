import type { AccountData } from './contracts';
export interface LikeOperation {
  id: string;
  revision: string;
  desired: boolean;
  observed: boolean | null;
  phase:
    | 'sending'
    | 'verifying'
    | 'uncertain'
    | 'confirmed'
    | 'rejected'
    | 'unchanged'
    | 'accepted_current_state';
  canAccept: boolean;
}
export interface LikeRecoveryView {
  total: number;
  offset: number;
  operations: LikeOperation[];
}
export type LikeRecoveryCommand =
  | { kind: 'reconcile'; id: string; revision: string }
  | { kind: 'accept_current'; id: string; revision: string; observed: boolean };
export interface LikeRecoveryClient {
  likeRecovery(epoch: string, offset: number): Promise<AccountData<LikeRecoveryView>>;
  recoverLike(epoch: string, command: LikeRecoveryCommand): Promise<AccountData<LikeOperation>>;
}
