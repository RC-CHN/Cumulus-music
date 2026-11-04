import type { AccountData } from './contracts';

export type ProfileRelationMode = 'following' | 'followers';
export type ProfileRelationKind = ProfileRelationMode | 'following_artists' | 'following_users';
export interface ProfileRelationTarget {
  id: string;
  owner: string;
  name: string;
  mode: ProfileRelationMode;
  isSelf: boolean;
  relationVisit: number;
}
export interface ProfileRelationCard {
  id: string;
  kind: 'user' | 'artist';
  name: string;
  remark: string | null;
  avatarUrl: string | null;
  signature: string | null;
  relationship: 'following' | 'mutual' | null;
}
export type ProfileRelationAccess =
  | { state: 'allowed'; isSelf: boolean }
  | { state: 'denied'; message: string };
export type ProfileRelationPage =
  | {
      state: 'ready';
      items: ProfileRelationCard[];
      continuation: string | null;
      recordCount: number | null;
    }
  | { state: 'denied'; message: string };

export interface ProfileRelationClient {
  profileRelationAccess(epoch: string, uid: string): Promise<AccountData<ProfileRelationAccess>>;
  profileRelationPage(
    epoch: string,
    uid: string,
    kind: ProfileRelationKind,
    continuation: string | null,
  ): Promise<AccountData<ProfileRelationPage>>;
}
