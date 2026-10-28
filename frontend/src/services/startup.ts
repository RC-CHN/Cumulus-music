import type { AccountData } from './contracts';
export interface StartupSettings {
  autoPlay: boolean;
  rememberPosition: boolean;
}
export interface StartupPreferencesView {
  revision: string | null;
  settings: StartupSettings;
  damaged: boolean;
}
export type StartupPreferencesCommand =
  | { kind: 'change'; expected_revision: string; settings: StartupSettings }
  | { kind: 'repair' };
export interface StartupPreferencesClient {
  startupPreferences(epoch: string): Promise<AccountData<StartupPreferencesView>>;
  startupPreferencesCommand(
    epoch: string,
    command: StartupPreferencesCommand,
  ): Promise<AccountData<StartupPreferencesView>>;
}
