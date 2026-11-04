export type EffectCatalogueKind = 'highly_recommended' | 'recommended' | 'more';

export interface EffectCatalogueItem {
  id: number;
  name: string;
  subtitle: string;
  membership?: 'vip' | 'svip';
  trial_remaining_ms?: number;
  light_artwork?: string;
  dark_artwork?: string;
}

export type EffectAccess = 'allowed' | 'trial' | 'unavailable' | 'unknown';

export interface EffectDeviceChoice {
  id: number;
  name: string;
}
export type EffectDeviceMaterialCheck =
  | 'no_material'
  | 'pending'
  | 'standalone_peq'
  | 'unsupported_graph';

export type EffectPeqPhase =
  | 'idle'
  | 'preparing'
  | 'prepared'
  | 'unsupported'
  | 'configured'
  | 'deferred'
  | 'paused_locally'
  | 'processor_failed'
  | 'recovery_required';

export type EffectRegistration =
  | 'registering'
  | 'registered'
  | 'start_uncertain'
  | 'start_rejected'
  | 'releasing'
  | 'released'
  | 'stop_uncertain'
  | 'stop_rejected';

export interface PendingEffectRegistration {
  id: number;
  phase: EffectRegistration;
}

export interface EffectPeqView {
  id: number;
  phase: EffectPeqPhase;
  attempt: string | null;
  release: string | null;
  registration: EffectRegistration | null;
  canStop: boolean;
  canResume?: boolean;
  canPause?: boolean;
  stopNeedsRetry: boolean;
  activeId?: number | null;
  issue?: string | null;
}

export interface EffectDirectoryClient {
  effectCatalogue(epoch: string, kind: EffectCatalogueKind): Promise<EffectCatalogueItem[]>;
  effectDetail(
    epoch: string,
    id: number,
  ): Promise<(EffectCatalogueItem & { access: EffectAccess }) | null>;
  effectPeqPending(epoch: string): Promise<PendingEffectRegistration[]>;
  effectPeqStatus(epoch: string, id: number): Promise<EffectPeqView>;
  effectPeqPrepare(epoch: string, id: number): Promise<EffectPeqView>;
  effectPeqActivate(epoch: string, id: number, attempt: string): Promise<EffectPeqView>;
  effectPeqResume(epoch: string, id: number, release: string): Promise<EffectPeqView>;
  effectPeqPause(epoch: string, id: number, release: string): Promise<EffectPeqView>;
  effectPeqStop(
    epoch: string,
    id: number,
    release: string,
    retryUnknown: boolean,
  ): Promise<EffectPeqView>;
  effectPeqReconcile(epoch: string, id: number): Promise<EffectPeqView>;
  effectDeviceBrands(epoch: string): Promise<EffectDeviceChoice[]>;
  effectDeviceModels(epoch: string, brand: number): Promise<EffectDeviceChoice[]>;
  effectDeviceMaterial(
    epoch: string,
    brand: number,
    model: number,
  ): Promise<EffectDeviceMaterialCheck>;
  effectDeviceApply(epoch: string, brand: number, model: number): Promise<void>;
  effectDeviceClear(epoch: string, brand: number, model: number): Promise<void>;
}
