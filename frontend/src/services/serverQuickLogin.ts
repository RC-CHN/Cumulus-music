import type { LoginStatus } from './auth.ts';
import type { AccountData } from './contracts.ts';

export type ServerQuickLoginPhase =
  | 'ready'
  | 'exchanging'
  | 'checking_identity'
  | 'identity_required'
  | 'result_unknown'
  | 'verification_required'
  | 'rejected'
  | 'authenticated'
  | 'cancelled';

export interface ServerQuickLoginOperationView {
  operationId: string;
  phase: ServerQuickLoginPhase;
  canReconcile: boolean;
  status: LoginStatus | null;
}

export type ServerQuickLoginCommand = 'execute' | 'reconcile' | 'cancel';

export interface ServerQuickLoginClient {
  beginServerQuickLogin(
    epoch: string,
    savedId: string,
    consent: boolean,
  ): Promise<AccountData<ServerQuickLoginOperationView>>;
  serverQuickLoginCommand(
    epoch: string,
    id: string,
    command: ServerQuickLoginCommand,
  ): Promise<AccountData<ServerQuickLoginOperationView>>;
}
