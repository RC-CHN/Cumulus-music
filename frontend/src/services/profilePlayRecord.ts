import type { AccountData, TrackView } from './contracts';

export type ProfilePlayRecordPeriod = 'week' | 'all';

export interface ProfilePlayRecordTarget {
  uid: string | null;
  name: string;
  period: ProfilePlayRecordPeriod;
}

export type ProfilePlayRecordView =
  | { state: 'denied' }
  | {
      state: 'ready';
      isSelf: boolean;
      items: { track: TrackView; playCount?: number }[];
    };

export interface ProfilePlayRecordClient {
  profilePlayRecord(
    epoch: string,
    uid: string | null,
    period: ProfilePlayRecordPeriod,
  ): Promise<AccountData<ProfilePlayRecordView>>;
}
