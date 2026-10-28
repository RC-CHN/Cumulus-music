import type { AccountData } from './contracts';
export interface OutputDevice {
  id: string;
  description: string;
}
export type OutputPhase =
  | 'dormant'
  | 'pending'
  | 'unsupported'
  | 'configured'
  | 'fallback'
  | 'failed';
export type OutputIssue =
  | 'missing_device'
  | 'initialization'
  | 'selection'
  | 'enumeration'
  | 'damaged_preferences'
  | 'storage';
export interface OutputEngine {
  kind: 'audio' | 'video';
  revision: string | null;
  phase: OutputPhase;
  issue: OutputIssue | null;
  configured: string | null;
  driver: string | null;
}
export interface OutputView {
  revision: string | null;
  selected: OutputDevice;
  damaged: boolean;
  supported: boolean | null;
  devices: OutputDevice[];
  engines: OutputEngine[];
}
export type OutputCommand =
  | { kind: 'select'; expected_revision: string; id: string }
  | { kind: 'retry'; expected_revision: string }
  | { kind: 'repair' };
export interface OutputClient {
  outputSettings(epoch: string, refresh: boolean): Promise<AccountData<OutputView>>;
  outputCommand(epoch: string, command: OutputCommand): Promise<AccountData<OutputView>>;
}
