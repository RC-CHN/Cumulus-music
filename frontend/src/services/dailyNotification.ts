import type { AccountData } from './contracts.ts';

export type DailyNotificationKind = 'daily' | 'copyright';

export interface DailyNotification {
  epoch: string;
  receipt: string;
  kind: DailyNotificationKind;
  playlistId: string | null;
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

export type DailyNotificationOutcome =
  | 'shown'
  | 'closed'
  | 'settings'
  | 'details'
  | 'played'
  | 'disabled_daily'
  | 'disabled_copyright';

export interface DailyNotificationPreference extends AccountData<boolean> {
  kind: DailyNotificationKind;
}

export interface DailyNotificationClient {
  dailyNotification(epoch: string): Promise<DailyNotification | null>;
  dailyNotificationCommand(
    epoch: string,
    receipt: string,
    action: DailyNotificationAction,
  ): Promise<DailyNotificationOutcome>;
  onDailyNotification(listener: (view: DailyNotification | null) => void): Promise<() => void>;
  dailyNotificationEnabled(epoch: string, kind: DailyNotificationKind): Promise<boolean | null>;
  setDailyNotificationEnabled(
    epoch: string,
    kind: DailyNotificationKind,
    enabled: boolean,
  ): Promise<boolean>;
  onDailyNotificationPreference(
    listener: (view: DailyNotificationPreference) => void,
  ): Promise<() => void>;
}
