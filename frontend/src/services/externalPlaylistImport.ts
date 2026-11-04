import type { AccountData } from './contracts';
import type { ExternalPlaylistInput } from './playlists';

export type ExternalImportAction =
  | { kind: 'inspect' }
  | { kind: 'prepare'; inputs: string[] }
  | { kind: 'cancel' | 'submit'; receipt: string };

export type ExternalImportOutcome =
  | { state: 'ready' }
  | { state: 'unknown' | 'accepted'; operation: string };

export interface ExternalImportView {
  authorized: boolean;
  outcome: ExternalImportOutcome;
  prepared: { receipt: string; name: string; links: ExternalPlaylistInput[] } | null;
}

export type ExternalImportAuthAction =
  | { kind: 'begin' }
  | { kind: 'poll' | 'reconcile' | 'cancel'; operation_id: string };

export type ExternalImportAuthState =
  | 'idle'
  | 'creating'
  | 'waiting_scan'
  | 'waiting_confirmation'
  | 'identity_pending'
  | 'authorized'
  | 'expired'
  | 'challenge'
  | 'wrong_account'
  | 'unconfirmed';

export interface ExternalImportAuthView {
  epoch: string;
  operationId: string | null;
  state: ExternalImportAuthState;
  busy: boolean;
  pollAfterMs: number | null;
  modules: boolean[][];
}

export interface ExternalPlaylistImportClient {
  externalPlaylistImport(
    epoch: string,
    action: ExternalImportAction,
  ): Promise<AccountData<ExternalImportView>>;
  externalPlaylistImportAuth(epoch: string): Promise<ExternalImportAuthView>;
  externalPlaylistImportAuthAction(
    epoch: string,
    action: ExternalImportAuthAction,
  ): Promise<ExternalImportAuthView>;
}
