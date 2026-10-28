import type { AccountData } from './contracts';
export type CloseBehavior = 'tray' | 'exit';
export interface WindowSettings {
  closeBehavior: CloseBehavior;
  rememberChoice: boolean;
}
export interface WindowPreferencesView {
  revision: string | null;
  settings: WindowSettings;
  damaged: boolean;
}
export type WindowPreferencesCommand =
  | { kind: 'change'; expected_revision: string; settings: WindowSettings }
  | { kind: 'repair' };
export interface WindowPreferencesClient {
  windowPreferences(epoch: string): Promise<AccountData<WindowPreferencesView>>;
  windowPreferencesCommand(
    epoch: string,
    command: WindowPreferencesCommand,
  ): Promise<AccountData<WindowPreferencesView>>;
}
export interface WindowLifecycleView {
  revision: string;
  installed: boolean;
  trayAvailable: boolean;
  busy: boolean;
  hidden: boolean;
  prompt: { id: string; epoch: string; preferences: WindowPreferencesView } | null;
  message: string | null;
}
export type WindowCloseCommand =
  | { kind: 'cancel'; id: string }
  | { kind: 'confirm'; id: string; behavior: CloseBehavior; remember: boolean };
export interface WindowLifecycleClient {
  windowLifecycle(): Promise<WindowLifecycleView>;
  windowCloseCommand(command: WindowCloseCommand): Promise<WindowLifecycleView>;
  onWindowLifecycle(handler: (view: WindowLifecycleView) => void): Promise<() => void>;
}
