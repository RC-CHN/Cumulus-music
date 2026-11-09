export type SystemMasterVolume =
  | { kind: 'unsupported' | 'unavailable' }
  | { kind: 'ready'; percent: number };

export interface SystemMasterVolumeClient {
  systemMasterVolume(): Promise<SystemMasterVolume>;
}

export type SystemSpatialSound =
  | { kind: 'unsupported' | 'unavailable' }
  | { kind: 'ready'; active: boolean };
export interface SystemSpatialSoundClient {
  systemSpatialSound(): Promise<SystemSpatialSound>;
}

export type VolumeDiagnosisDismissal = 'auto_close' | 'manual_close' | 'jump_settings';

export interface VolumeDiagnosisVisit {
  epoch: string;
  before: number;
  after: number;
}
export interface VolumeDiagnosisClient {
  checkVolumeDiagnosis(visit: VolumeDiagnosisVisit): Promise<string | null>;
  dismissVolumeDiagnosis(
    epoch: string,
    receipt: string,
    action: VolumeDiagnosisDismissal,
  ): Promise<void>;
}
