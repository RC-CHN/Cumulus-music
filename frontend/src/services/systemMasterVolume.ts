export type SystemMasterVolume =
  | { kind: 'unsupported' | 'unavailable' }
  | { kind: 'ready'; percent: number };

export interface SystemMasterVolumeClient {
  systemMasterVolume(): Promise<SystemMasterVolume>;
}
