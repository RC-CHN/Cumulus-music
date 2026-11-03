import type { AccountData } from './contracts';

export interface QueueClickPreferencesView {
  revision: string | null;
  damaged: boolean;
  replaceQueue: boolean;
}

export type QueueClickPreferencesCommand =
  | { kind: 'change'; expected_revision: string; replace_queue: boolean }
  | { kind: 'repair' };

export interface QueueClickPreferencesClient {
  queueClickPreferences(epoch: string): Promise<AccountData<QueueClickPreferencesView>>;
  queueClickPreferencesCommand(
    epoch: string,
    command: QueueClickPreferencesCommand,
  ): Promise<AccountData<QueueClickPreferencesView>>;
}
