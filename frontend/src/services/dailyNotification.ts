import type { AccountData } from './contracts.ts';

export interface DailyNotification {
  epoch: string;
  receipt: string;
  day: number;
  description: string;
  songs: { name: string; artists: string; coverUrl: string | null }[];
  remainingMs: number;
  shown: boolean;
}

export type DailyNotificationAction =
  | 'shown'
  | 'close'
  | 'snooze'
  | 'disable'
  | 'settings'
  | 'details'
  | 'play';

export type DailyNotificationOutcome = 'shown' | 'closed' | 'settings' | 'details' | 'played';

export interface DailyNotificationClient {
  dailyNotification(epoch: string): Promise<DailyNotification | null>;
  dailyNotificationCommand(
    epoch: string,
    receipt: string,
    action: DailyNotificationAction,
  ): Promise<DailyNotificationOutcome>;
  onDailyNotification(listener: (view: DailyNotification | null) => void): Promise<() => void>;
  dailyNotificationEnabled(epoch: string): Promise<boolean>;
  setDailyNotificationEnabled(epoch: string, enabled: boolean): Promise<boolean>;
  onDailyNotificationPreference(
    listener: (view: AccountData<boolean>) => void,
  ): Promise<() => void>;
}
