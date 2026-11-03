export type EffectCatalogueKind = 'highly_recommended' | 'recommended' | 'more';

export interface EffectCatalogueItem {
  id: number;
  name: string;
  subtitle: string;
}

export type EffectAccess = 'allowed' | 'trial' | 'unavailable' | 'unknown';

export interface EffectDirectoryClient {
  effectCatalogue(epoch: string, kind: EffectCatalogueKind): Promise<EffectCatalogueItem[]>;
  effectDetail(
    epoch: string,
    id: number,
  ): Promise<(EffectCatalogueItem & { access: EffectAccess }) | null>;
}
