export type EffectCatalogueKind = 'highly_recommended' | 'recommended' | 'more';

export interface EffectCatalogueItem {
  id: number;
  name: string;
  subtitle: string;
  light_artwork?: string;
  dark_artwork?: string;
}

export type EffectAccess = 'allowed' | 'trial' | 'unavailable' | 'unknown';

export interface EffectDeviceChoice {
  id: number;
  name: string;
}

export interface EffectDirectoryClient {
  effectCatalogue(epoch: string, kind: EffectCatalogueKind): Promise<EffectCatalogueItem[]>;
  effectDetail(
    epoch: string,
    id: number,
  ): Promise<(EffectCatalogueItem & { access: EffectAccess }) | null>;
  effectDeviceBrands(epoch: string): Promise<EffectDeviceChoice[]>;
  effectDeviceModels(epoch: string, brand: number): Promise<EffectDeviceChoice[]>;
}
