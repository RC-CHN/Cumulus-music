import type { AccountData } from './contracts';

export type RemoteSkinImageKind = 'cover' | 'preview';
export interface RemoteSkinCard {
  key: number;
  name: string;
  entitlement: 'vinyl' | 'super_vinyl' | null;
  corner: 'popular' | 'new' | 'limited' | null;
  hasCover: boolean;
  hasPreview: boolean;
}
export interface RemoteSkinCatalog {
  revision: string;
  groups: { name: string; cards: RemoteSkinCard[] }[];
}
export interface RemoteSkinClient {
  remoteSkinCatalog(epoch: string): Promise<AccountData<RemoteSkinCatalog>>;
  remoteSkinImage(
    epoch: string,
    revision: string,
    key: number,
    kind: RemoteSkinImageKind,
  ): Promise<AccountData<string>>;
}
