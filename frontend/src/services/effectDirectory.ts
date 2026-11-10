import type { DeviceParametricView } from './player';

export type EffectCatalogueKind = 'highly_recommended' | 'recommended' | 'more';

export interface EffectCatalogueItem {
  id: number;
  name: string;
  subtitle: string;
  membership?: 'vip' | 'svip';
  trial_remaining_ms?: number;
  light_artwork?: string;
  dark_artwork?: string;
  newly_added?: boolean;
}

export type EffectDirectorySection = 'selection' | 'more' | 'legacy_recommended' | 'legacy_all';

export interface EffectDirectoryOverview {
  cohortRevision: string;
  layout: 'legacy' | 'modern';
  groups: { key: EffectDirectorySection; items: EffectCatalogueItem[] }[];
  partial: boolean;
}

export type EffectAccess = 'allowed' | 'trial' | 'unavailable' | 'unknown';

export interface EffectDeviceChoice {
  id: number;
  name: string;
}

export interface RememberedHeadphone {
  brand: number;
  model: number;
  revision: string;
}

export function headphoneTuningLabel(status: DeviceParametricView['status'], processing = false) {
  return {
    configured: processing ? '耳机调音正在处理音频' : '耳机调音已配置',
    bypassed_for_source: '当前音源已旁路耳机调音',
    deferred: '音频加载后启用耳机调音',
    paused_locally: '耳机调音已暂时停用',
    detach_required: '耳机调音尚未关闭，请重试关闭',
    processor_failed: '耳机调音失败，已停用',
  }[status];
}

export interface DeviceTuningComparison {
  brand: number;
  model: number;
  activationId: string;
  playId: string;
  receipt: string;
  paused: boolean;
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
  | 'bypassed_for_source'
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
  effectDirectoryOverview(epoch: string): Promise<EffectDirectoryOverview>;
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
  effectDeviceRemembered(epoch: string): Promise<RememberedHeadphone | null>;
  effectDeviceRestore(epoch: string, revision: string): Promise<void>;
  effectDeviceCompare(epoch: string, comparison: DeviceTuningComparison): Promise<void>;
  effectDeviceClear(
    epoch: string,
    brand: number,
    model: number,
    activationId: string,
  ): Promise<void>;
}
