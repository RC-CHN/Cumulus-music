import type { AccountData } from './contracts';

export type ProfileFollowCommand = { kind: 'change'; expected: boolean } | { kind: 'reconcile' };
export type ProfileFollowStatus =
  | 'ready'
  | 'followed'
  | 'unfollowed'
  | 'unchanged'
  | 'uncertain'
  | 'limit'
  | 'privacy'
  | 'blocked'
  | 'rejected';
export interface ProfileFollowRelation {
  followed: boolean;
  mutual: boolean;
  followersCount: number | null;
}
export interface ProfileFollowView {
  relation: ProfileFollowRelation | null;
  pending: boolean;
  status: ProfileFollowStatus;
}
export interface ProfileFollowClient {
  profileFollow(
    epoch: string,
    uid: string,
    command: ProfileFollowCommand,
  ): Promise<AccountData<ProfileFollowView>>;
}
