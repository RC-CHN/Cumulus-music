import type { AccountData, TrackView } from './contracts';

export type ProfilePlayRecordPeriod = 'week' | 'all';

export interface ProfilePlayRecordTarget {
  uid: string | null;
  name: string;
  period: ProfilePlayRecordPeriod;
}

export type ProfilePlayRecordRemovalAction = 'remove' | 'check';
export type ProfilePlayRecordRemovalOutcome = 'removed' | 'uncertain' | 'not_listed';
export interface ProfilePlayRecordEntry {
  track: TrackView;
  playCount?: number;
  removalUncertain?: boolean;
}

export type ProfilePlayRecordView =
  | { state: 'denied' }
  | {
      state: 'ready';
      isSelf: boolean;
      revision?: string;
      items: ProfilePlayRecordEntry[];
    };

export interface ProfilePlayRecordClient {
  profilePlayRecord(
    epoch: string,
    uid: string | null,
    period: ProfilePlayRecordPeriod,
  ): Promise<AccountData<ProfilePlayRecordView>>;
  removeProfilePlayRecord(
    epoch: string,
    period: ProfilePlayRecordPeriod,
    songId: string,
    revision: string,
    action: ProfilePlayRecordRemovalAction,
  ): Promise<AccountData<ProfilePlayRecordRemovalOutcome>>;
}
